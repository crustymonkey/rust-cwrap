extern crate random_number;

use super::cmdstate;
use super::errors::lockfile;
use super::helpers::{format_ts, SyslogHelper};
use super::smtp::{send_email, SMTPOptions};
use super::statefile::StateFile;
use crate::sleep_ms;
use crate::Args;
use log::{debug, error};
use random_number::random;
use serde_json;
use std::path::PathBuf;
use std::process::exit;

pub struct RunManager {
    cmd_state: cmdstate::CmdState,
    syslog: Option<SyslogHelper>,
    statefile: StateFile,
    fuzz: usize,
    num_retries: usize,
    retry_secs: usize,
    ignore_retry_fails: bool,
    timeout: usize,
    quiet: bool,
    num_fails: usize,
    backoff: bool,
    first_fail: bool,
    smtp_options: SMTPOptions,
}

impl RunManager {
    pub fn new(args: &Args) -> Self {
        let mut statefile = StateFile::from_strs(
            &StateFile::gen_name(&args.cmd, args.bash_string),
            &args.state_dir,
        );

        if let Some(f) = &args.lock_file {
            statefile.overwrite_lockfile(PathBuf::from(f));
        }

        // First, we try and load the CmdState from disk and create it
        // otherwise
        let cmd_state = match cmdstate::CmdState::load(&statefile) {
            Ok(Some(v)) => v,
            Ok(None) => cmdstate::CmdState::new(&args.cmd, args.bash_string),
            Err(e) => {
                panic!(
                    "Error loading command state from statefile {}: {}",
                    statefile.full_p.to_str().unwrap(),
                    e
                );
            }
        };

        let mut syslog = None;
        if args.syslog {
            syslog = Some(SyslogHelper::new(&args.syslog_pri, &args.syslog_fac));
        }

        let smtp_options = SMTPOptions::from_args(args);

        return Self {
            cmd_state: cmd_state,
            syslog: syslog,
            statefile: statefile,
            fuzz: args.fuzz,
            num_retries: args.num_retries,
            retry_secs: args.retry_secs,
            ignore_retry_fails: args.ignore_retry_fails,
            timeout: args.timeout,
            quiet: args.quiet,
            num_fails: args.num_fails,
            backoff: args.backoff,
            first_fail: args.first_fail,
            smtp_options: smtp_options,
        };
    }

    pub fn run_instance(&mut self, lock: bool) {
        let fuzz = self.fuzz as u64;
        if self.fuzz > 0 {
            // Sleep for a random bit here
            let sl_time: u64 = random!(..=fuzz);
            debug!("Sleeping (fuzz) for {} secs", sl_time);
            sleep_ms!(sl_time * 1000);
        }

        if lock {
            if let Err(e) = self.lock() {
                if !self.ignore_retry_fails {
                    error!(
                        "Could not get lock to run instance in {} retries: {}",
                        self.num_retries, e,
                    );
                    exit(1);
                }
            }
        }

        let run = cmdstate::CmdRun::run(&self.cmd_state, self.cmd_state.bash_string, self.timeout);
        if run.exit_code != 0 || run.rust_err.is_some() {
            // We have a failure of some sort here
            self.handle_failure(run);
        } else {
            if !self.quiet {
                self.print_success_report(&run);
            }
            self.cmd_state.reset();
        }

        if let Err(e) = self.cmd_state.save(&self.statefile) {
            error!("Serialize failure: {}", e);
        }
    }

    /// Generate and print a report if necessary, per the cli opts
    fn handle_failure(&mut self, run: cmdstate::CmdRun) {
        self.cmd_state.num_fails += 1;

        if self.syslog.is_some() {
            // Need to serialize the command run and write that
            match serde_json::to_string(&run) {
                Ok(data) => self.log(&format!(
                    "CWRAP FAILURE for `{}`: {}",
                    self.cmd_state.cli_to_string(),
                    data,
                )),
                Err(e) => self.log(&format!("Error serializing run error: {}", e)),
            }
        }

        // Now, determine whether we print a report or not.  I could do this as
        // a single OR statement, but it's a bit more readable as if/else if
        if self.backoff && self.backoff_match() {
            self.print_failure_report(&run);
        } else if self.cmd_state.num_fails % self.num_fails == 0 && !self.backoff {
            self.print_failure_report(&run);
        } else if self.first_fail && self.cmd_state.num_fails == 1 {
            self.print_failure_report(&run);
        } else {
            // Finally, increment the failure and push the failure into the
            // failures vec if we haven't run a report
            self.cmd_state.failures.push(run);
        }
    }

    fn print_failure_report(&mut self, run: &cmdstate::CmdRun) {
        let mut output = String::new();
        output.push_str(&format!(
            "The specified number of failures, {}, has been reached \
                for the following command, which has failed {} times in a \
                row: {}\n\nFAILURES:\n",
            self.num_fails,
            self.cmd_state.num_fails,
            &self.cmd_state.cli_to_string(),
        ));

        // First, we print out the previous runs
        for fail in &self.cmd_state.failures {
            self.add_run_report(&mut output, fail);
        }

        self.add_run_report(&mut output, run);

        if self.smtp_options.send_email {
            if let Err(e) = send_email(&output, &self.smtp_options) {
                print!(
                    "*** Failed to send the email using internal transport ***\nError: {}\n",
                    e
                );
            }
        }

        // Print if we are not sending an email unless also normal output is set
        if !self.smtp_options.send_email || self.smtp_options.also_normal_output {
            print!("{}", output);
        }

        // And finally, reset the command state
        self.cmd_state.reset_runs();
    }

    fn print_success_report(&self, run: &cmdstate::CmdRun) {
        let mut output = String::new();
        output.push_str("The command has run successfully!\n\n");
        self.add_run_report(&mut output, run);

        print!("{}", output);
    }

    /// This will add to the building of a string for the failure report for a
    /// single run
    fn add_run_report(&self, rep: &mut String, fail: &cmdstate::CmdRun) {
        let f_div = "=====\n";
        let out_div = "-----\n";
        rep.push_str(f_div);
        rep.push_str(&format!("Command: {}\n", &self.cmd_state.cli_to_string()));
        rep.push_str(&format!("Start Time: {}\n", format_ts(fail.start_time)));
        rep.push_str(&format!("Run Time (seconds): {:.2}\n", fail.run_time));
        rep.push_str("Exit Code: ");
        if let Some(e) = &fail.rust_err {
            rep.push_str(&format!("Internal Error: {}\n", e));
        } else {
            rep.push_str(&format!("{}\n", fail.exit_code));
        }

        if !fail.stdout.is_empty() {
            rep.push_str("\n");
            rep.push_str(&format!("STDOUT:\n{}", out_div));
            rep.push_str(&fail.stdout);
            rep.push_str("\n");
            rep.push_str(out_div);
        }

        if !fail.stderr.is_empty() {
            rep.push_str("\n");
            rep.push_str(&format!("STDERR:\n{}", out_div));
            rep.push_str(&fail.stderr);
            rep.push_str("\n");
            rep.push_str(out_div);
        }
        rep.push_str(f_div);
    }

    fn backoff_match(&self) -> bool {
        let mut count = self.num_fails;
        while count <= self.cmd_state.num_fails {
            if count == self.cmd_state.num_fails {
                return true;
            }

            count *= 2;
        }

        return false;
    }

    /// This will create the lockfile based on cli options that are set
    pub fn lock(&self) -> lockfile::Result<()> {
        let tries = self.num_retries as i64;
        let ret_secs = self.retry_secs as u64;

        // The default for num_retries is 0, which is no retries, which is
        // why I'm setting this to -1 to allow it to run at least once
        let mut try_count: i64 = -1;
        let mut ret: lockfile::Result<()> = Ok(());

        while tries > try_count {
            debug!("Attempting to acquire lock to run");
            ret = self.statefile.lock();
            if ret.is_err() && tries > 0 {
                try_count += 1;
                sleep_ms!(ret_secs * 1000);
            } else {
                break;
            }
        }
        if ret.is_ok() {
            debug!("Lock successfully acquired!");
        }

        return ret;
    }

    pub fn unlock(&self) -> lockfile::Result<()> {
        return self.statefile.unlock();
    }

    pub fn get_statefile_clone(&self) -> StateFile {
        return self.statefile.clone();
    }

    /// A shortcut to log to the syslogger if syslogging is set,
    /// otherwise this just goes to a black hole
    fn log(&mut self, msg: &str) {
        if !self.syslog.is_none() {
            self.syslog.as_mut().unwrap().log(msg);
        }
    }
}

impl Drop for RunManager {
    fn drop(&mut self) {
        debug!("Running the drop for the manager");
        if let Err(e) = self.unlock() {
            error!("Error removing the lockfile!!: {}", e);
        }
    }
}
