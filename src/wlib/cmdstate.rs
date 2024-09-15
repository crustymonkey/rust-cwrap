use log::debug;
use serde::{Serialize, Deserialize};
use serde_json;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use std::sync::Arc;
use crate::sleep_ms;
use super::statefile::StateFile;
use super::errors::serialize;


/// This will manage the overall state of running the sub-commands
#[derive(Serialize, Deserialize)]
pub struct CmdState {
    pub cmd: Vec<String>,
    pub num_fails: usize,
    pub failures: Vec<CmdRun>,
}

impl CmdState {
    pub fn new(cmd: &Vec<String>) -> Self {
        return Self {
            cmd: cmd.clone(),
            num_fails: 0,
            failures: vec![],
        };
    }

    /// Attempt to load the statefile from disk, will return the deserialized
    /// version or None
    pub fn load(sf: &StateFile) -> serialize::Result<Option<Self>> {
        let sfs = match sf.get_contents_string() {
            Ok(data) => Some(data),
            Err(e) => {
                debug!("Failed to get the contents from the statefile: {}", e);
                None
            },
        };

        if sfs.is_none() {
            // The file likely doesn't exist yet
            return Ok(None);
        }

        return match serde_json::from_str(&sfs.unwrap()) {
            Ok(v) => Ok(Some(v)),
            Err(e) => Err(serialize::SerDeError::new(
                format!("Failed to deserialize the content: {}", e)
            )),
        };
    }

    /// Reset both the number of failures and the vec of CmdRun.  This
    /// should be called when a good run occurs
    pub fn reset(&mut self) {
        self.num_fails = 0;
        self.failures = Vec::new();
    }

    /// This is called after a report is printed so we aren't storing
    /// infinite runs
    pub fn reset_runs(&mut self) {
        self.failures = Vec::new();
    }

    pub fn save(&self, sf: &StateFile) -> serialize::Result<()> {
        let ser_data = match serde_json::to_string(self) {
            Ok(data) => data,
            Err(e) => {
                return Err(serialize::SerDeError::new(
                    format!("Error serializing data: {}", e)
                ));
            },
        };

        return match sf.write_contents(ser_data) {
            Err(e) => Err(serialize::SerDeError::new(
                format!("Error writing serialized data: {}", e)
            )),
            _ => Ok(()),
        }
    }

    pub fn cli_to_string(&self) -> String {
        return self.cmd.join(" ").to_string();
    }
}

/// This handles the state for the last command run
#[derive(Serialize, Deserialize)]
pub struct CmdRun {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub start_time: f64,
    pub run_time: f64,
    pub rust_err: Option<String>,
}

impl CmdRun {
    /// Do a run of a command and return a CmdRun struct as the result
    pub fn run(cmd: &CmdState, args: Arc<ArgMatches<'static>>) -> Self {
        let start = SystemTime::now();

        debug!("Spawning the child process for {}", cmd.cli_to_string());
        let mut proc;

        if args.is_present("bash-string") {
            // We have to run this as a string under bash instead
            proc = match Command::new("bash")
                    .args(&["-c".to_string(), cmd.cmd.clone()])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn() {
                Ok(child) => child,
                Err(e) => {
                    return CmdRun::rust_err(
                        format!("Failed to spawn child: {}", e)
                    );
                },
            };
        } else {
            proc = match Command::new(&cmd.cmd)
                    .args(&cmd.cmd_args)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn() {
                Ok(child) => child,
                Err(e) => {
                    return CmdRun::rust_err(
                        format!("Failed to spawn child: {}", e)
                    );
                },
            };
        }

        debug!("Child started with pid: {}", proc.id());

        // Convert to millis
        let timeout = value_t!(args, "timeout", u64).unwrap() * 1000;

        let mut run_time = 0;
        if timeout > 0 {
            // Need to handle timeouts here with try_wait on the proc
            while run_time < timeout {
                match &proc.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) => {
                        run_time += 100;
                        sleep_ms!(100);
                    },
                    Err(e) => {
                        return CmdRun::rust_err(
                            format!("Failure to spawn child: {}", e)
                        );
                    },
                }
            }
        }

        // Check to see if we went over time
        if timeout > 0 && run_time >= timeout {
            match &proc.try_wait() {
                Ok(None) => {
                    debug!("Timeout exceeded, killing the subprocess");

                    match proc.kill() {
                        Ok(_) => return Self {
                            exit_code: -1,
                            stdout: String::new(),
                            stderr: String::new(),
                            start_time: start
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs_f64(),
                            run_time: SystemTime::now()
                                .duration_since(start)
                                .unwrap()
                                .as_secs_f64(),
                            rust_err: Some(format!(
                                "Command reached timeout of {} secs",
                                timeout / 1000,
                            )),
                        },
                        Err(e) => return CmdRun::rust_err(
                            format!("Failed to kill subprocess! {}", e)
                        ),
                    }
                },
                _ => (),
            }
        }

        let output = match proc.wait_with_output() {
            Ok(out) => out,
            Err(e) => {
                return CmdRun::rust_err(
                    format!("Failure running child: {}", e)
                );
            },
        };

        let total_run_time = SystemTime::now().duration_since(start).unwrap();

        return Self {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            start_time: start.duration_since(UNIX_EPOCH).unwrap().as_secs_f64(),
            run_time: total_run_time.as_secs_f64(),
            rust_err: None,
        };
    }

    fn rust_err(err_msg: String) -> Self {
        return Self {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            start_time: 0.0,
            run_time: 0.0,
            rust_err: Some(err_msg),
        };
    }
}
