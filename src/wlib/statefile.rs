extern crate md5;

use std::convert::From;
use std::fs::{File, OpenOptions, remove_file};
use std::os::unix::fs::OpenOptionsExt;
use std::io::{self, Write, Read};
use std::path::PathBuf;
use std::process;
use super::errors::lockfile;
use super::helpers::sanitize_path;

#[allow(dead_code)]
#[derive(Clone)]
pub struct StateFile {
    name: String,
    base_path: PathBuf,
    full_p: PathBuf,
    lockfile: PathBuf,  // This will be base_path + name + .lock
}

impl StateFile {
    pub fn from_strs(name: &str, base_path: &str) -> Self {
        let lock_name = name.to_string() + ".lock";
        let bp = PathBuf::from(base_path);

        let mut full_p = bp.clone();
        full_p.push(name);

        let mut lockfile = PathBuf::from("/dev/shm/");
        lockfile.push(lock_name);

        return StateFile {
            name: name.to_string(),
            base_path: bp,
            full_p: full_p,
            lockfile: lockfile,
        };
    }

    /// Generate a name for the statefile, which is:
    ///     <command basename>.<md5 of full cli>
    pub fn gen_name(cmd: &str, args: &Vec<String>, is_bash: bool) -> String {
        let mut cli = cmd.to_string();
        if args.len() > 0 {
            cli.push_str(" ");
            cli.push_str(&args.join(" "));
        }

        let hash_str = format!("{:x}", md5::compute(cli.as_bytes()));
        // This will get set based on whether it's a bash string or separate
        // args
        let mut ret;

        if is_bash {
            ret = sanitize_path(cli.split(" ").collect::<Vec<&str>>()[0], '-');
        } else {
            ret = sanitize_path(cmd, '-');
        }

        ret.push_str(".");
        ret.push_str(&hash_str);

        return ret;
    }

    /// Set a specific lockfile for this run rather than use the auto-gen file
    pub fn overwrite_lockfile(&mut self, p: PathBuf) {
        self.lockfile = p;
    }

    pub fn get_contents_string(&self) -> io::Result<String> {
        let mut fp = File::open(&self.full_p)?;
        let mut contents = String::new();
        fp.read_to_string(&mut contents)?;

        return Ok(contents);
    }

    pub fn write_contents(&self, contents: String) -> io::Result<()> {
        let mut fp = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&self.full_p)?;
        let mut buf: Vec<u8> = contents.into_bytes();
        fp.write_all(&mut buf)?;

        return Ok(());
    }

    pub fn lock(&self) -> lockfile::Result<()> {
        if self.lockfile.exists() {
            return Err(lockfile::LockError::new(
                format!("Lockfile exists: {}", self.lockfile.display())));
        }
        
        // Write the current pid to the lockfile and handle errors
        match OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&self.lockfile) {
            Ok(mut fp) => {
                let mut b: Vec<u8> = process::id()
                    .to_string()
                    .as_bytes()
                    .to_vec();
                if let Err(e) = fp.write_all(&mut b) {
                    return Err(lockfile::LockError::new(
                        format!("Failed to write to lockfile: {}", e)
                    ));
                }
            },
            Err(e) => return Err(lockfile::LockError::new(
                format!("Failed to create lockfile: {}", e)
            )),
        }

        debug!("Created lockfile at {}", &self.lockfile.display());

        return Ok(());
    }

    pub fn unlock(&self) -> lockfile::Result<()> {
        if self.lockfile.exists() {
            debug!("Removing lockfile at: {}", &self.lockfile.display());
            if let Err(e) = remove_file(&self.lockfile) {
                return Err(lockfile::LockError::new(
                    format!("Failure removing the lock file: {}", e)
                ));
            }
        }
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_statefile_from_strs() {
        let name = "bin-cat.abcdef";
        let dir = "/var/tmp";
        let s = StateFile::from_strs(name, dir);
        let lockname = name.to_string() + ".lock";

        assert_eq!(s.name, name.to_string());
        assert_eq!(s.base_path, PathBuf::from(dir));
        let mut tmp = PathBuf::from(dir);
        tmp.push(name);
        assert_eq!(s.full_p, tmp);
        tmp = PathBuf::from("/dev/shm");
        tmp.push(lockname);
        assert_eq!(s.lockfile, tmp);
    }
}
