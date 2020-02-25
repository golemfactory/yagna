use structopt::StructOpt;


#[derive(StructOpt)]
pub enum CmdLine {
    Publish {
        path: PathBuf,
    }
}



fn main() -> Result<()> {
    let cmd_args = CmdLine::from_args();
    match cmd_args {
        Publish {path} => {

        }
    }
}

