# rust-cwrap
A rust version of [cron-wrap](https://github.com/crustymonkey/cron-wrap)

## IMPORTANT!
Version 0.2.0 is **breaking upgrade**.  You will have to remove all previous state files you have on disk as replacing the cwrap binary with a 0.1.x binary without removing state files will cause a crash (by design).  The state on disk has changed between versions.

## About
This is mostly the same implementation as [cron-wrap](https://github.com/crustymonkey/cron-wrap), but without the SMTP options.  Those *may* be added
later, but for now, this should be fully usable for most cases.

See `cwrap --help` for all of the various options here.

**An important note about CLI parsing**: If you have options for your command,
you **must** terminate the options for `cwrap` with a `--`.  For example,
if you were going to run `grep -R something /path/to/dir/*` and you wanted to
set a cwrap option like `--num-fails`, you would do it like this:
```bash
cwrap --num-fails 3 -- grep -R something /path/to/dir
```

Unfortunately, [clap](https://docs.rs/clap/2.33.3/clap/) will gobble up anything
that looks like an option no matter where it is, unless the options are
terminated.