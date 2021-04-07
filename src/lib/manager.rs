extern crate random_number;

use log::{debug, error};
use serde_json;
use clap::{ArgMatches, value_t};
use std::sync::Arc;
use std::path::PathBuf;
use super::cmdstate;
use super::helpers::{SyslogHelper, format_ts};
use crate::sleep_ms;
use super::statefile::StateFile;
use super::errors::lockfile;
use random_number::random;

pub struct RunManager {
    args: Arc<ArgMatches<'static>>,
    cmd_state: cmdstate::CmdState,
    syslog: Option<SyslogHelper>,
    statefile: StateFile,
}

impl RunManager {
    pub fn new(args: Arc<ArgMatches<'static>>) -> Self {
        let cmd = args.value_of("CMD").unwrap().to_owned();
        let cmd_args = match args.values_of_lossy("ARGS") {
            Some(v) => v,
            None => Vec::new(),
        };

        let mut statefile = StateFile::from_strs(
            &StateFile::gen_name(
                &cmd,
                &cmd_args,
                args.is_present("bash-string"),
            ),
            args.value_of("state_dir").unwrap(),
        );

        if let Some(f) = args.value_of("lock-file") {
            statefile.overwrite_lockfile(PathBuf::from(f));
        }
        
        // First, we try and load the CmdState from disk and create it
        // otherwise
        let cmd_state = match cmdstate::CmdState::load(&statefile) {
            Ok(Some(v)) => v,
            Ok(None) => cmdstate::CmdState::new(
                cmd.clone(),
                cmd_args.clone(),
            ),
            Err(e) => {
                panic!("Error loading command state from disk: {}", e);
            }
        };

        let mut syslog = None;
        if args.is_present("syslog") {
            syslog = Some(SyslogHelper::new(
                args.value_of("syslog_pri").unwrap(),
                args.value_of("syslog_fac").unwrap(),
            ));
        }

        return Self {
            args: args.clone(),
            cmd_state: cmd_state,
            syslog: syslog,
            statefile: statefile
        };
    }

    pub fn run_instance(&mut self) {
        // This is a hack to make the value_t macro work properly.  Not a big
        // deal as it's just another ref count
        let a = self.args.clone();

        let fuzz = value_t!(a, "fuzz", u64).unwrap();
        if fuzz > 0 {
            // Sleep for a random bit here
            let sl_time: u64 = random!(..=fuzz);
            debug!("Sleeping (fuzz) for {} secs", sl_time);
            sleep_ms!(sl_time * 1000);
        }

        let run = cmdstate::CmdRun::run(&self.cmd_state, self.args.clone());
        if run.exit_code != 0 || run.rust_err.is_some() {
            // We have a failure of some sort here
            self.handle_failure(run);
        } else {
            if !a.is_present("quiet") {
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
        let a = self.args.clone();
        self.cmd_state.num_fails += 1;
        let fail_thresh: usize = value_t!(a, "num_fails", usize).ok().unwrap();

        if a.is_present("syslog") {
            // Need to serialize the command run and write that
            match serde_json::to_string(&run) {
                Ok(data) => self.log(
                    &format!(
                        "CWRAP FAILURE for `{}`: {}",
                        self.cmd_state.cli_to_string(),
                        data,
                    )
                ),
                Err(e) => self.log(
                    &format!("Error serializing run error: {}", e)
                ),
            }
        }

        // Now, determine whether we print a report or not.  I could do this as
        // a single OR statement, but it's a bit more readable as if/else if
        if a.is_present("backoff") && self.backoff_match() {
            self.print_failure_report(&run);
        } else if self.cmd_state.num_fails % fail_thresh == 0 
                && !a.is_present("backoff") {
            self.print_failure_report(&run);
        } else if a.is_present("first-fail") && self.cmd_state.num_fails == 1 {
            self.print_failure_report(&run);
        } else {
            // Finally, increment the failure and push the failure into the
            // failures vec if we haven't run a report
            self.cmd_state.failures.push(run);
        }
    }

    fn print_failure_report(&mut self, run: &cmdstate::CmdRun) {
        let mut output = String::new();
        output.push_str(
            &format!("The specified number of failures, {}, has been reached \
                for the following command, which has failed {} times in a \
                row: {}\n\nFAILURES:\n",
                self.args.value_of("num_fails").unwrap(),
                self.cmd_state.num_fails,
                self.cmd_state.cmd,
            )
        );

        // First, we print out the previous runs
        for fail in &self.cmd_state.failures {
            self.add_run_report(&mut output, fail);
        }

        self.add_run_report(&mut output, run);

        print!("{}", output);

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
        rep.push_str(
            &format!("Command: {}\n", &self.cmd_state.cli_to_string())
        );
        rep.push_str(
            &format!("Start Time: {}\n", format_ts(fail.start_time))
        );
        rep.push_str(&format!("Run Time (seconds): {:.2}\n", fail.run_time));
        rep.push_str("Exit Code: ");
        if let Some(e) = &fail.rust_err {
            rep.push_str(&format!("Internal Error: {}\n", e));
        } else {
            rep.push_str(&format!("{}\n", fail.exit_code));
        }

        if fail.stdout.len() > 0 {
            rep.push_str("\n");
            rep.push_str(&format!("STDOUT:\n{}", out_div));
            rep.push_str(&fail.stdout);
            rep.push_str("\n");
            rep.push_str(out_div);
        }

        if fail.stderr.len() > 0 {
            rep.push_str("\n");
            rep.push_str(&format!("STDERR:\n{}", out_div));
            rep.push_str(&fail.stderr);
            rep.push_str("\n");
            rep.push_str(out_div);
        }
        rep.push_str(f_div);
    }

    fn backoff_match(&self) -> bool {
        let a = self.args.clone();
        let mut count: usize = value_t!(a, "num_fails", usize).ok().unwrap();
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
        let a = self.args.clone();
        let tries = value_t!(a, "num_retries", u32).ok().unwrap();
        let ret_secs = value_t!(a, "retry_secs", u64).ok().unwrap();
        // The default for num_retries is 0, which is no retries, which is
        // why I'm setting this to -1 to allow it to run at least once
        let mut try_count: i64 = -1;
        let mut ret: lockfile::Result<()> = Ok(());

        while i64::from(tries) > try_count {
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
        if let Err(e) = self.unlock() {
            error!("Error removing the lockfile!!: {}", e);
        }
    }
}