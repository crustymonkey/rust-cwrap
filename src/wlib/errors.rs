pub mod lockfile {
    use std::fmt;

    pub type Result<T> = std::result::Result<T, LockError>;

    pub struct LockError {
        msg: String,
    }

    impl LockError {
        pub fn new(msg: String) -> Self {
            return Self { msg };
        }
    }

    impl fmt::Display for LockError {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            return write!(f, "Failed to lock file: {}", self.msg);
        }
    }
}

pub mod loc_syslog {
    use std::fmt;

    pub type Result<T> = std::result::Result<T, SyslogError>;

    pub struct SyslogError {
        msg: String,
    }

    impl SyslogError {
        pub fn new(msg: String) -> Self {
            return Self { msg };
        }
    }

    impl fmt::Display for SyslogError {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            return write!(f, "Syslog error: {}", self.msg);
        }
    }
}

pub mod serialize {
    use std::fmt;
    pub type Result<T> = std::result::Result<T, SerDeError>;

    pub struct SerDeError {
        msg: String,
    }

    impl SerDeError {
        pub fn new(msg: String) -> Self {
            return Self { msg };
        }
    }

    impl fmt::Display for SerDeError {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            return write!(f, "Serialization/deserialization error: {}", self.msg);
        }
    }
}
