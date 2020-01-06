#[macro_use]
extern crate prettytable;
use std::ffi::{OsStr, OsString};
use std::fs::{copy, remove_file, File};
use std::io::{Read, Result, Write};
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use chrono::{DateTime, Local, Utc};
use prettytable::{Cell, Row, Table};
use serde::{Deserialize, Serialize};

mod wall_clock;

const STATE_FILE_PREFIX: &str = ".easy-deploy";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct DeployedBin {
    id: u128,
    time: DateTime<Utc>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct TargetState {
    deployments: Vec<DeployedBin>,
    current: Option<u128>,
    target: PathBuf,
}

impl TargetState {
    fn new(target: PathBuf) -> Self {
        TargetState {
            deployments: Vec::new(),
            current: None,
            target,
        }
    }

    fn next_deployment(&self) -> u128 {
        match self.deployments.iter().map(|d| d.id).max() {
            None => 0,
            Some(max) => max + 1,
        }
    }

    fn add_deployment(mut self, new_id: u128, time: DateTime<Utc>) -> Self {
        self.deployments.push(DeployedBin { id: new_id, time });
        TargetState {
            deployments: self.deployments,
            current: Some(new_id),
            target: self.target,
        }
    }

    fn dump(self) -> Result<()> {
        let state_path = get_state_path(&self.target);
        let serialized_state = serde_json::to_string(&self)?;
        let mut state_file = File::create(state_path)?;
        state_file.write_all(serialized_state.as_bytes())
    }

    fn load(target: PathBuf) -> Self {
        let state_path = get_state_path(&target);
        if state_path.exists() {
            let mut state_file = File::open(state_path).unwrap();
            let mut serialized_state = String::new();
            state_file.read_to_string(&mut serialized_state).unwrap();
            serde_json::from_str(&serialized_state).unwrap()
        } else {
            Self::new(target)
        }
    }

    fn pretty_print(&self, output: &mut impl Write) -> Result<usize> {
        let mut table = Table::new();

        table.add_row(row!["id", "timestamp", "current"]);
        self.deployments.iter().for_each(|d| {
            let current_symbol = match self.current {
                None => " ",
                Some(current) => {
                    if current == d.id {
                        "*"
                    } else {
                        " "
                    }
                }
            };
            let time: DateTime<Local> = DateTime::from(d.time);
            table.add_row(row![
                d.id.to_string(),
                time.format("%Y-%d-%m %H:%M:%S"),
                current_symbol
            ]);
        });
        table.print(output)
    }
}

fn modify_filename(path: &PathBuf, f: impl Fn(&OsStr) -> OsString) -> PathBuf {
    let name = path.file_name().expect("invalid path, can't get file name");
    return path.with_file_name(f(name));
}

fn add_to_os_string(s: &str, os_string: &OsStr) -> OsString {
    let mut result = OsString::new();
    result.push(OsStr::new(s));
    result.push(os_string);
    result
}

fn get_hidden_target(target: &PathBuf, id: u128) -> PathBuf {
    modify_filename(target, |name| add_to_os_string(&format!("{}_", id), name))
}

fn get_state_path(target: &PathBuf) -> PathBuf {
    modify_filename(target, |name| add_to_os_string(STATE_FILE_PREFIX, name))
}

fn deploy_with_state(state: TargetState, exe: &PathBuf, now: DateTime<Utc>) -> Result<TargetState> {
    let next_id = state.next_deployment();
    let hidden_path = get_hidden_target(&state.target, next_id);
    let _ = copy(exe, &hidden_path)?;
    if state.target.exists() {
        // need to remove target because symlinks fail if the dst exists
        let _ = remove_file(&state.target)?;
    }
    let _ = symlink(hidden_path, &state.target)?;
    return Ok(state.add_deployment(next_id, now));
}

fn deploy_internal(
    exe: &PathBuf,
    target: PathBuf,
    clock: &impl wall_clock::WallClock,
) -> Result<()> {
    let state = TargetState::load(target);
    let now = clock.now();
    let state = deploy_with_state(state, exe, now)?;
    state.dump()
}

pub fn deploy(exe: &PathBuf, target: PathBuf) -> Result<()> {
    deploy_internal(exe, target, &wall_clock::SYSTEM_TIME)
}

pub fn list(target: PathBuf) -> Result<()> {
    let state = TargetState::load(target);
    state.pretty_print(&mut std::io::stdout()).map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::{Cursor, Read, Write};
    use std::path::PathBuf;
    use std::str::FromStr;

    use chrono::{DateTime, Duration};
    #[cfg(test)]
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::{deploy_internal, deploy_with_state, wall_clock, DeployedBin, TargetState};

    fn assert_contents_matches(file: &PathBuf, contents: &str) {
        let mut buf = String::new();
        let mut result_handle = File::open(file).unwrap();
        let _ = result_handle.read_to_string(&mut buf).unwrap();
        assert_eq!(buf, contents);
    }

    fn create_file(path: &PathBuf, contents: &str) {
        let mut file = File::create(path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
    }

    fn setup_test_temp_dir() -> (PathBuf, PathBuf) {
        let dir = tempdir().unwrap().into_path();
        let mut target = dir.clone();
        target.push("my_bin");
        let mut to_deploy = dir.clone();
        to_deploy.push("to_deploy.exe");
        (to_deploy, target)
    }

    #[test]
    fn deploying_with_state_copies_contents() {
        let (to_deploy, target) = setup_test_temp_dir();
        let state = TargetState::new(target);
        let time = DateTime::from_str("2020-01-01T12:25:00-00:00").unwrap();

        let text1: &str = "some contents";
        create_file(&to_deploy, text1);

        let state = deploy_with_state(state, &to_deploy, time).unwrap();
        assert_contents_matches(&state.target, text1);

        let text2: &str = "contents changed";
        create_file(&to_deploy, text2);

        let state = deploy_with_state(state, &to_deploy, time).unwrap();
        assert_contents_matches(&state.target, text2);

        assert_eq!(
            state.deployments,
            vec![DeployedBin { id: 0, time }, DeployedBin { id: 1, time }]
        );
    }

    #[test]
    fn full_deploy_copies_contents() {
        let (to_deploy, target) = setup_test_temp_dir();
        let clock =
            wall_clock::FakeTime::new(DateTime::from_str("2020-01-01T04:50:00-00:00").unwrap());

        let text1: &str = "some contents";
        create_file(&to_deploy, text1);

        deploy_internal(&to_deploy, target.clone(), &clock).unwrap();
        assert_contents_matches(&target, text1);

        let clock = clock.advance(Duration::minutes(2));
        let text2: &str = "contents changed";
        create_file(&to_deploy, text2);

        deploy_internal(&to_deploy, target.clone(), &clock).unwrap();
        assert_contents_matches(&target, text2);

        let expected_state = TargetState {
            deployments: vec![
                DeployedBin {
                    id: 0,
                    time: DateTime::from_str("2020-01-01T04:50:00Z").unwrap(),
                },
                DeployedBin {
                    id: 1,
                    time: DateTime::from_str("2020-01-01T04:52:00Z").unwrap(),
                },
            ],
            current: Some(1),
            target: target.clone(),
        };
        let state = TargetState::load(target);
        assert_eq!(expected_state, state);
    }
}
