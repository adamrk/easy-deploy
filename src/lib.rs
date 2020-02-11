#[macro_use]
extern crate prettytable;
#[cfg(test)]
#[macro_use]
extern crate maplit;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs::{copy, remove_file, File};
use std::io::{Read, Result, Write};
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use chrono::{DateTime, Local, Utc};
use prettytable::Table;
use serde::{Deserialize, Serialize};

mod wall_clock;

const STATE_FILE_PREFIX: &str = ".easy-deploy";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct DeployedBinV1 {
    time: DateTime<Utc>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct DeployedBinV2 {
    time: DateTime<Utc>,
    message: String,
}

impl DeployedBinV1 {
    fn to_v2(self) -> DeployedBinV2 {
        DeployedBinV2 {
            time: self.time,
            message: "".to_string(),
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct TargetStateV1 {
    deployments: HashMap<u128, DeployedBinV1>,
    current: Option<u128>,
    target: PathBuf,
}

impl TargetStateV1 {
    fn to_v2(self) -> TargetState {
        TargetState {
            deployments: self
                .deployments
                .into_iter()
                .map(|(k, v)| (k, v.to_v2()))
                .collect(),
            current: self.current,
            target: self.target,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct TargetStateV2 {
    deployments: HashMap<u128, DeployedBinV2>,
    current: Option<u128>,
    target: PathBuf,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum VersionedTargetState {
    V1(TargetStateV1),
    V2(TargetStateV2),
}

type TargetState = TargetStateV2;

impl VersionedTargetState {
    fn to_latest(self) -> TargetState {
        match self {
            Self::V1(v1) => v1.to_v2(),
            Self::V2(v2) => v2,
        }
    }
}

impl TargetState {
    fn new(target: PathBuf) -> Self {
        TargetState {
            deployments: HashMap::new(),
            current: None,
            target,
        }
    }

    fn next_deployment(&self) -> u128 {
        match self.deployments.iter().map(|(id, _)| id).max() {
            None => 0,
            Some(max) => max + 1,
        }
    }

    fn add_deployment(mut self, new_id: u128, time: DateTime<Utc>, message: String) -> Self {
        self.deployments
            .insert(new_id, DeployedBinV2 { time, message });
        TargetState {
            deployments: self.deployments,
            current: Some(new_id),
            target: self.target,
        }
    }

    fn to_versioned(self) -> VersionedTargetState {
        VersionedTargetState::V2(self)
    }

    fn dump(self) -> Result<()> {
        let state_path = get_state_path(&self.target);
        let serialized_state = serde_json::to_string(&self.to_versioned())?;
        let mut state_file = File::create(state_path)?;
        state_file.write_all(serialized_state.as_bytes())
    }

    fn load(target: PathBuf) -> Self {
        let state_path = get_state_path(&target);
        if state_path.exists() {
            let mut state_file = File::open(state_path).unwrap();
            let mut serialized_state = String::new();
            state_file.read_to_string(&mut serialized_state).unwrap();
            serde_json::from_str::<VersionedTargetState>(&serialized_state)
                .unwrap()
                .to_latest()
        } else {
            Self::new(target)
        }
    }

    fn pretty_print(&self, output: &mut impl Write) -> Result<usize> {
        let mut table = Table::new();

        table.add_row(row!["id", "timestamp", "current"]);
        self.deployments.iter().for_each(|(id, d)| {
            let current_symbol = self
                .current
                .map_or(" ", |current| if current == *id { "*" } else { " " });
            let time: DateTime<Local> = DateTime::from(d.time);
            table.add_row(row![
                id.to_string(),
                time.format("%Y-%d-%m %H:%M:%S"),
                current_symbol
            ]);
        });
        table.print(output)
    }
}

fn modify_filename(path: &PathBuf, f: impl Fn(&OsStr) -> OsString) -> PathBuf {
    let name = path.file_name().expect("invalid path, can't get file name");
    path.with_file_name(f(name))
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

fn deploy_with_state(
    state: TargetState,
    exe: &PathBuf,
    now: DateTime<Utc>,
    message: String,
) -> Result<TargetState> {
    let next_id = state.next_deployment();
    let hidden_path = get_hidden_target(&state.target, next_id);
    let _ = copy(exe, &hidden_path)?;
    if state.target.exists() {
        // need to remove target because symlinks fail if the dst exists
        let _ = remove_file(&state.target)?;
    }
    let _ = symlink(hidden_path, &state.target)?;
    Ok(state.add_deployment(next_id, now, message))
}

fn deploy_internal(
    exe: &PathBuf,
    target: PathBuf,
    clock: &impl wall_clock::WallClock,
    message: String,
) -> Result<()> {
    let state = TargetState::load(target);
    let now = clock.now();
    let state = deploy_with_state(state, exe, now, message)?;
    state.dump()
}

pub fn deploy(exe: &PathBuf, target: PathBuf, message: String) -> Result<()> {
    deploy_internal(exe, target, &wall_clock::SYSTEM_TIME, message)
}

pub fn list(target: PathBuf) -> Result<()> {
    let state = TargetState::load(target);
    state.pretty_print(&mut std::io::stdout()).map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::{Read, Write};
    use std::path::PathBuf;
    use std::str::FromStr;

    use chrono::{DateTime, Duration};
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::{
        deploy_internal, deploy_with_state, get_state_path,
        wall_clock::{FakeTime, WallClock},
        DeployedBinV1, DeployedBinV2, TargetState, TargetStateV1, VersionedTargetState,
    };

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
        let message = "deploy";

        let text1: &str = "some contents";
        create_file(&to_deploy, text1);

        let state = deploy_with_state(state, &to_deploy, time, String::from(message)).unwrap();
        assert_contents_matches(&state.target, text1);

        let text2: &str = "contents changed";
        create_file(&to_deploy, text2);

        let state = deploy_with_state(state, &to_deploy, time, String::from(message)).unwrap();
        assert_contents_matches(&state.target, text2);

        assert_eq!(
            state.deployments,
            hashmap![
                0 => DeployedBinV2 { time, message: String::from(message) },
                1 => DeployedBinV2 { time, message: String::from(message) },
            ]
        );
    }

    #[test]
    fn full_deploy_copies_contents() {
        let (to_deploy, target) = setup_test_temp_dir();
        let clock = FakeTime::new(DateTime::from_str("2020-01-01T04:50:00-00:00").unwrap());

        let text1: &str = "some contents";
        create_file(&to_deploy, text1);

        deploy_internal(&to_deploy, target.clone(), &clock, String::from("deploy1")).unwrap();
        assert_contents_matches(&target, text1);

        let clock = clock.advance(Duration::minutes(2));
        let text2: &str = "contents changed";
        create_file(&to_deploy, text2);

        deploy_internal(&to_deploy, target.clone(), &clock, String::from("deploy2")).unwrap();
        assert_contents_matches(&target, text2);

        let expected_state = TargetState {
            deployments: hashmap! {
                0 => DeployedBinV2 {
                    time: DateTime::from_str("2020-01-01T04:50:00Z").unwrap(),
                    message: String::from("deploy1")
                },
                1 => DeployedBinV2 {
                    time: DateTime::from_str("2020-01-01T04:52:00Z").unwrap(),
                    message: String::from("deploy2")
                },
            },
            current: Some(1),
            target: target.clone(),
        };
        let state = TargetState::load(target);
        assert_eq!(expected_state, state);
    }

    #[test]
    fn full_deploy_upgrades_state() {
        let (to_deploy, target) = setup_test_temp_dir();

        let clock = FakeTime::new(DateTime::from_str("2020-01-01T04:50:00-00:00").unwrap());

        let old_state = TargetStateV1 {
            deployments: hashmap! {
                0 => DeployedBinV1 {
                    time: clock.now(),
                }
            },
            current: Some(0),
            target: target.clone(),
        };

        let state_path = get_state_path(&target);
        let serialized_state = serde_json::to_string(&VersionedTargetState::V1(old_state)).unwrap();
        let mut state_file = File::create(state_path).unwrap();
        state_file.write_all(serialized_state.as_bytes()).unwrap();

        let text: &str = "contents";
        create_file(&to_deploy, text);
        let clock = clock.advance(Duration::minutes(5));

        deploy_internal(&to_deploy, target.clone(), &clock, String::from("deploy")).unwrap();

        let expected_state = TargetState {
            deployments: hashmap! {
                0 => DeployedBinV2 {
                    time: DateTime::from_str("2020-01-01T04:50:00Z").unwrap(),
                    message: String::from("")
                },
                1 => DeployedBinV2 {
                    time: DateTime::from_str("2020-01-01T04:55:00Z").unwrap(),
                    message: String::from("deploy")
                },
            },
            current: Some(1),
            target: target.clone(),
        };
        let state = TargetState::load(target);
        assert_eq!(expected_state, state);
    }
}
