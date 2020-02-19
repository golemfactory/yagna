use crate::error::Error;
use crate::Result;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio_util::codec::{FramedRead, LinesCodec};

pub struct Process {
    pub proc: Option<Child>,
    pub stdout: Option<FramedRead<ChildStdout, LinesCodec>>,
    pub stderr: Option<FramedRead<ChildStderr, LinesCodec>>,
}

impl Process {
    pub fn spawn(mut cmd: Command) -> Result<Self> {
        let mut proc = cmd.spawn()?;

        let stdout = match proc.stdout.take() {
            Some(stdout) => FramedRead::new(stdout, LinesCodec::new()),
            None => {
                proc.kill()?;
                return Err(Error::ProcessError);
            }
        };
        let stderr = match proc.stderr.take() {
            Some(stderr) => FramedRead::new(stderr, LinesCodec::new()),
            None => {
                proc.kill()?;
                return Err(Error::ProcessError);
            }
        };

        Ok(Process {
            proc: Some(proc),
            stdout: Some(stdout),
            stderr: Some(stderr),
        })
    }
}
