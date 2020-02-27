use std::path::PathBuf;

use clap::{App, Arg, SubCommand};

use easy_deploy;

fn main() {
    let target_arg = Arg::with_name("TARGET")
        .help("target deploy location")
        .required(true);

    let message_arg = Arg::with_name("MESSAGE")
        .help("deploy comment")
        .required(false);

    let matches = App::new("easy-deploy")
        .about("simple deploy")
        .subcommand(
            SubCommand::with_name("deploy")
                .about("deploys file")
                .arg(
                    Arg::with_name("SOURCE")
                        .help("file to deploy")
                        .required(true)
                        .index(1),
                )
                .arg(target_arg.clone().index(2))
                .arg(message_arg.clone().index(3)),
        )
        .subcommand(
            SubCommand::with_name("list")
                .about("list deployments")
                .arg(target_arg.clone().index(1)),
        )
        .get_matches();
    let result = if let Some(matches) = matches.subcommand_matches("deploy") {
        easy_deploy::deploy(
            &PathBuf::from(matches.value_of("SOURCE").unwrap()),
            PathBuf::from(matches.value_of("TARGET").unwrap()),
            String::from(matches.value_of("MESSAGE").unwrap_or("")),
        )
    } else if let Some(matches) = matches.subcommand_matches("list") {
        easy_deploy::list(PathBuf::from(matches.value_of("TARGET").unwrap()))
    } else {
        Ok(())
    };
    match result {
        Err(error) => println!("Error {}", error),
        Ok(()) => (),
    }
}
