use crate::Args;
use anyhow::Result;
use hostname;
use lettre::{message::header::ContentType, Message, SmtpTransport, Transport};
use std::path::PathBuf;
use std::{fs::File, io::Read};
use users::{get_current_uid, get_user_by_uid};

pub struct SMTPOptions {
    pub send_email: bool,
    username: Option<String>,
    password: Option<String>,
    subject: String,
    smtp_server: String,
    smtp_port: usize,
    pub also_normal_output: bool,
    email_from: String,
    recipient: Vec<String>,
    tls: bool,
    starttls: bool,
}

impl SMTPOptions {
    /// Create a set of options
    pub fn new(
        send_email: bool,
        username: Option<String>,
        password: Option<String>,
        subject: String,
        smtp_server: String,
        smtp_port: usize,
        also_normal_output: bool,
        email_from: Option<String>,
        recipient: Vec<String>,
        tls: bool,
        starttls: bool,
    ) -> Self {
        let email_from = Self::generate_from_addr(email_from);
        return Self {
            send_email,
            username,
            password,
            subject,
            smtp_server,
            smtp_port,
            also_normal_output,
            email_from,
            recipient,
            tls,
            starttls,
        };
    }
    /// Extract the SMTP options from the command-line args
    pub fn from_args(args: &Args) -> Self {
        let mut username = args.username.clone();
        let mut password = args.password.clone();

        if let Some(path) = &args.creds_file {
            // We have the creds in a file, let's grab those and populate our
            // variables
            let (uname, passw) = Self::parse_creds(path).unwrap();
            username = Some(uname);
            password = Some(passw);
        }

        let mut recip: Vec<String> = vec![];

        if args.send_mail && args.recipient.is_none() {
            panic!("Invalid options, if you wish to send email directly, you must specify at least 1 recipient.");
        } else if let Some(tmp) = args.recipient.clone() {
            recip = tmp;
        }

        return Self {
            send_email: args.send_mail,
            username: username,
            password: password,
            subject: args.subject.clone(),
            smtp_server: args.smtp_server.clone(),
            smtp_port: args.smtp_port,
            also_normal_output: args.also_normal_output,
            email_from: Self::generate_from_addr(args.email_from.clone()),
            recipient: recip,
            tls: args.tls,
            starttls: args.starttls,
        };
    }

    /// Static method for parsing the creds file
    pub fn parse_creds(path: &PathBuf) -> Result<(String, String)> {
        let mut file = File::open(path)?;
        let mut buf: Vec<u8> = vec![];
        file.read_to_end(&mut buf)?;

        let contents = String::from_utf8(buf)?;
        let (username, password) = contents.split_once(':').unwrap();

        return Ok((username.to_string(), password.to_string()));
    }

    /// This will generate a from address by using the executing user and
    /// the system hostname
    pub fn generate_from_addr(email_from: Option<String>) -> String {
        // If the address was specified, just return it
        if let Some(from) = email_from {
            return from;
        }

        // Otherwise, we have to generate the from address from the user and
        // hostname
        let user = get_user_by_uid(get_current_uid()).unwrap();
        let hostname = hostname::get().unwrap();
        return format!(
            "{}@{}",
            user.name().to_str().unwrap(),
            &hostname.into_string().unwrap()
        );
    }

    /// Return an smtp url for use with lettre SMTPTransport::from_url()
    pub fn smtp_url(&self) -> String {
        // Start building out the url
        let mut url = "smtp".to_string();
        if self.tls {
            url.push_str("s");
        }
        url.push_str("://");

        if self.username.is_some() {
            url.push_str(&format!(
                "{}:{}@",
                self.username.clone().unwrap().as_str(),
                self.password.clone().unwrap_or("".to_string()).as_str()
            ));
        }

        url.push_str(&format!("{}:{}", &self.smtp_server, &self.smtp_port));

        if self.starttls {
            url.push_str("?tls=required");
        }

        return url;
    }
}

/// Convenience function for the sending of the email.
pub fn send_email(body: &str, opts: &SMTPOptions) -> Result<()> {
    if !opts.send_email {
        return Ok(());
    }

    let mut builder = Message::builder()
        .from(opts.email_from.as_str().parse()?)
        .reply_to(opts.email_from.as_str().parse()?)
        .to(opts.recipient[0].parse()?)
        .subject(opts.subject.clone())
        .header(ContentType::TEXT_PLAIN);

    // Add the rest of the recipients
    for cc in &opts.recipient[1..] {
        builder = builder.cc(cc.parse()?);
    }

    // Adding the body will finalize the message
    let message = builder.body(body.to_string())?;

    // Now we create the transport and send the email
    let mailer = SmtpTransport::from_url(&opts.smtp_url())?.build();

    mailer.send(&message)?;

    return Ok(());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{remove_file, OpenOptions};
    use std::io::Write;

    fn build_test_opts() -> SMTPOptions {
        return SMTPOptions::new(
            true,
            Some("monkey".to_string()),
            Some("password".to_string()),
            "Test Subject".to_string(),
            "smtp.example.com".to_string(),
            25,
            false,
            Some("user@example.com".to_string()),
            vec!["user2@example.com".to_string()],
            false,
            true,
        );
    }

    #[test]
    fn test_smtp_url() {
        let mut opts = build_test_opts();

        assert_eq!(
            "smtp://monkey:password@smtp.example.com:25?tls=required".to_string(),
            opts.smtp_url()
        );

        opts.tls = true;
        opts.starttls = false;

        assert_eq!(
            "smtps://monkey:password@smtp.example.com:25".to_string(),
            opts.smtp_url()
        );

        opts.username = None;
        assert_eq!("smtps://smtp.example.com:25".to_string(), opts.smtp_url());
    }

    #[test]
    fn test_parse_creds() {
        let fname = "/tmp/test-creds";
        let path = PathBuf::from(fname);
        let uname = "user";
        let password = "password";

        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .open(fname)
                .unwrap();
            let buf = format!("{}:{}", uname, password);
            file.write(buf.as_bytes()).unwrap();
        }

        let (user, pass) = SMTPOptions::parse_creds(&path).unwrap();

        assert_eq!(uname, &user);
        assert_eq!(password, &pass);

        remove_file(path).unwrap();
    }
}
