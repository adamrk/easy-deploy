use std::path::PathBuf;

use structopt::StructOpt;

use easy_deploy;

#[derive(StructOpt)]
#[structopt(
    name = "easy-deploy",
    about = "Deploy the easy way to a shared location"
)]
enum Command {
    #[structopt(name = "deploy", about = "Deploy a file")]
    Deploy {
        #[structopt(name = "FILE", help = "File to deploy", required = true)]
        source: PathBuf,

        #[structopt(name = "TARGET", help = "Location to deploy to", required = true)]
        target: PathBuf,

        #[structopt(long, name = "MESSAGE", help = "deploy message", required = false)]
        message: String,
    },
    #[structopt(name = "rollback", about = "Rollback deployment")]
    Rollback {
        #[structopt(name = "TARGET", help = "Target to rollback", required = true)]
        target: PathBuf,

        #[structopt(long, name = "MESSAGE", help = "rollback message", required = false)]
        message: String,

        #[structopt(
            long,
            name = "VERSION",
            help = "version id to rollback to (defaults to previous version)"
        )]
        version: Option<u32>,
    },
    #[structopt(name = "list", about = "List deployed versions")]
    List {
        #[structopt(name = "TARGET", help = "Deployed target to show", required = true)]
        target: PathBuf,
    },
}

fn main() {
    let command = Command::from_args();
    match command {
        Command::Deploy {
            source,
            target,
            message,
        } => easy_deploy::deploy(&source, target, message).unwrap(),
        Command::Rollback {
            target,
            message,
            version,
        } => easy_deploy::rollback(target, message, version).unwrap(),
        Command::List { target } => easy_deploy::list(target).unwrap(),
    }
}
