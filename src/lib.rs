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

const STATE_FILE_PREFIX: &str = ".easy-deploy_";
const MAX_VERSIONS_TO_KEEP: usize = 10;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct DeployedBinV1 {
    time: DateTime<Utc>,
    message: String,
    original_id: u32,
}

/// Invariant: All keys in the map are <= current and current is a key in the map (unless it is None).
/// Invariant: All keys in the map are <= current and current is a key in the map (unless it is None).
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct TargetStateV1 {
    deployments: HashMap<u32, DeployedBinV1>,
    current: Option<u32>,
    target: PathBuf,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum VersionedTargetState {
    V1(TargetStateV1),
}

type TargetState = TargetStateV1;

impl VersionedTargetState {
    fn to_latest(self) -> TargetState {
        match self {
            Self::V1(v1) => v1,
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

    fn next_deployment(&self) -> u32 {
        match self.deployments.iter().map(|(id, _)| id).max() {
            None => 0,
            Some(max) => max + 1,
        }
    }

    fn add_deployment(
        mut self,
        new_id: u32,
        time: DateTime<Utc>,
        message: String,
        original_id: Option<u32>,
    ) -> Self {
        self.deployments.insert(
            new_id,
            DeployedBinV1 {
                time,
                message,
                original_id: original_id.unwrap_or(new_id),
            },
        );
        TargetState {
            deployments: self.deployments,
            current: Some(new_id),
            target: self.target,
        }
    }

    fn to_versioned(self) -> VersionedTargetState {
        VersionedTargetState::V1(self)
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

        table.add_row(row!["id", "message", "timestamp", "current"]);
        let ordered_ids = self.order_ids();
        ordered_ids.iter().for_each(|id| {
            let deploy = self.deployments.get(id).unwrap();
            let current_symbol = self
                .current
                .map_or(" ", |current| if current == *id { "*" } else { " " });
            let time: DateTime<Local> = DateTime::from(deploy.time);
            table.add_row(row![
                deploy.original_id.to_string(),
                deploy.message,
                time.format("%Y-%d-%m %H:%M:%S"),
                current_symbol
            ]);
        });
        table.print(output)
    }

    fn order_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.deployments.keys().map(|x| *x).collect();
        ids.sort_by(|x, y| y.cmp(x));
        ids
    }

    fn garbage_collect(&mut self) -> Vec<u32> {
        let ordered_ids = self.order_ids();
        let to_drop: Vec<u32> = ordered_ids
            .iter()
            .skip(MAX_VERSIONS_TO_KEEP)
            .map(|x| *x)
            .collect();
        let dropping: &Vec<u32> = to_drop.as_ref();
        for id in dropping {
            self.deployments.remove(&id);
        }
        to_drop
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

fn get_hidden_target(target: &PathBuf, id: u32) -> PathBuf {
    modify_filename(target, |name| add_to_os_string(&format!(".{}_", id), name))
}

fn get_state_path(target: &PathBuf) -> PathBuf {
    modify_filename(target, |name| add_to_os_string(STATE_FILE_PREFIX, name))
}

fn delete_garbage(mut state: TargetState) -> Result<TargetState> {
    let garbage = state.garbage_collect();
    for id in garbage {
        let path = get_hidden_target(&state.target, id);
        if path.exists() {
            let _ = remove_file(path)?;
        }
    }
    Ok(state)
}

fn deploy_with_state(
    state: TargetState,
    exe: &PathBuf,
    now: DateTime<Utc>,
    message: String,
    original_id: Option<u32>,
) -> Result<TargetState> {
    let next_id = state.next_deployment();
    let hidden_path = get_hidden_target(&state.target, next_id);
    let _ = copy(exe, &hidden_path)?;
    if state.target.exists() {
        // need to remove target because symlinks fail if the dst exists
        let _ = remove_file(&state.target)?;
    }
    let _ = symlink(hidden_path, &state.target)?;
    let state = state.add_deployment(next_id, now, message, original_id);
    delete_garbage(state)
}

fn deploy_internal(
    exe: &PathBuf,
    target: PathBuf,
    clock: &impl wall_clock::WallClock,
    message: String,
    original_id: Option<u32>,
) -> Result<()> {
    let state = TargetState::load(target);
    let now = clock.now();
    let state = deploy_with_state(state, exe, now, message, original_id)?;
    state.dump()
}

fn rollback_internal(
    target: PathBuf,
    clock: &impl wall_clock::WallClock,
    message: String,
    rollback_id: Option<u32>,
) -> Result<()> {
    let state = TargetState::load(target);
    let rollback_id = rollback_id.unwrap_or_else(|| {
        let ordered_ids = state.order_ids();
        match ordered_ids.get(1) {
            None => panic!("no previous deploy"),
            Some(id) => *id,
        }
    });
    let to_deploy = get_hidden_target(&state.target, rollback_id);
    deploy_internal(&to_deploy, state.target, clock, message, Some(rollback_id))
}

pub fn deploy(exe: &PathBuf, target: PathBuf, message: String) -> Result<()> {
    deploy_internal(exe, target, &wall_clock::SYSTEM_TIME, message, None)
}

pub fn list(target: PathBuf) -> Result<()> {
    let state = TargetState::load(target);
    state.pretty_print(&mut std::io::stdout()).map(|_| ())
}

pub fn rollback(target: PathBuf, message: String, rollback_id: Option<u32>) -> Result<()> {
    rollback_internal(target, &wall_clock::SYSTEM_TIME, message, rollback_id)
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
        deploy_internal, deploy_with_state, rollback_internal, wall_clock::FakeTime, DeployedBinV1,
        TargetState, MAX_VERSIONS_TO_KEEP,
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

        let state =
            deploy_with_state(state, &to_deploy, time, String::from(message), None).unwrap();
        assert_contents_matches(&state.target, text1);

        let text2: &str = "contents changed";
        create_file(&to_deploy, text2);

        let state =
            deploy_with_state(state, &to_deploy, time, String::from(message), None).unwrap();
        assert_contents_matches(&state.target, text2);

        assert_eq!(
            state.deployments,
            hashmap![
                0 => DeployedBinV1 { time, message: String::from(message), original_id: 0 },
                1 => DeployedBinV1 { time, message: String::from(message), original_id: 1 },
            ]
        );
    }

    #[test]
    fn full_deploy_copies_contents() {
        let (to_deploy, target) = setup_test_temp_dir();
        let clock = FakeTime::new(DateTime::from_str("2020-01-01T04:50:00-00:00").unwrap());

        let text1: &str = "some contents";
        create_file(&to_deploy, text1);

        deploy_internal(
            &to_deploy,
            target.clone(),
            &clock,
            String::from("deploy1"),
            None,
        )
        .unwrap();
        assert_contents_matches(&target, text1);

        let clock = clock.advance(Duration::minutes(2));
        let text2: &str = "contents changed";
        create_file(&to_deploy, text2);

        deploy_internal(
            &to_deploy,
            target.clone(),
            &clock,
            String::from("deploy2"),
            None,
        )
        .unwrap();
        assert_contents_matches(&target, text2);

        let expected_state = TargetState {
            deployments: hashmap! {
                0 => DeployedBinV1 {
                    time: DateTime::from_str("2020-01-01T04:50:00Z").unwrap(),
                    message: String::from("deploy1"),
                    original_id: 0,
                },
                1 => DeployedBinV1 {
                    time: DateTime::from_str("2020-01-01T04:52:00Z").unwrap(),
                    message: String::from("deploy2"),
                    original_id: 1,
                },
            },
            current: Some(1),
            target: target.clone(),
        };
        let state = TargetState::load(target);
        assert_eq!(expected_state, state);
    }

    #[test]
    fn rollback_works() {
        let (to_deploy, target) = setup_test_temp_dir();
        let clock = FakeTime::new(DateTime::from_str("2020-01-01T04:50:00-00:00").unwrap());

        let text1: &str = "some contents";
        create_file(&to_deploy, text1);
        deploy_internal(
            &to_deploy,
            target.clone(),
            &clock,
            String::from("deploy1"),
            None,
        )
        .unwrap();

        let clock = clock.advance(Duration::minutes(5));
        let text2: &str = "contents changed";
        create_file(&to_deploy, text2);
        deploy_internal(
            &to_deploy,
            target.clone(),
            &clock,
            String::from("deploy2"),
            None,
        )
        .unwrap();

        let clock = clock.advance(Duration::minutes(5));
        let text3: &str = "contents the third";
        create_file(&to_deploy, text3);
        deploy_internal(
            &to_deploy,
            target.clone(),
            &clock,
            String::from("deploy3"),
            None,
        )
        .unwrap();

        let state = TargetState::load(target.clone());
        assert_eq!(
            vec![2, 1, 0],
            state.order_ids().iter().map(|x| *x).collect::<Vec<u32>>()
        );

        let clock = clock.advance(Duration::minutes(5));
        rollback_internal(target.clone(), &clock, String::from("rollback"), None).unwrap();

        let expected_state = TargetState {
            deployments: hashmap! {
                0 => DeployedBinV1 {
                    time: DateTime::from_str("2020-01-01T04:50:00Z").unwrap(),
                    message: String::from("deploy1"),
                    original_id: 0,
                },
                1 => DeployedBinV1 {
                    time: DateTime::from_str("2020-01-01T04:55:00Z").unwrap(),
                    message: String::from("deploy2"),
                    original_id: 1,
                },
                2 => DeployedBinV1 {
                    time: DateTime::from_str("2020-01-01T05:00:00Z").unwrap(),
                    message: String::from("deploy3"),
                    original_id: 2,
                },
                3 => DeployedBinV1 {
                    time: DateTime::from_str("2020-01-01T05:05:00Z").unwrap(),
                    message: String::from("rollback"),
                    original_id: 1,
                },
            },
            current: Some(3),
            target: target.clone(),
        };
        let state = TargetState::load(target.clone());
        assert_eq!(expected_state, state);
        assert_contents_matches(&state.target, "contents changed")
    }

    #[test]
    fn garbage_collection_works() {
        let (to_deploy, target) = setup_test_temp_dir();
        let clock = FakeTime::new(DateTime::from_str("2020-01-01T04:50:00-00:00").unwrap());

        for _ in 0..20 {
            let text1: &str = "some contents";
            create_file(&to_deploy, text1);
            deploy_internal(
                &to_deploy,
                target.clone(),
                &clock,
                String::from("deploy1"),
                None,
            )
            .unwrap();
        }

        let state = TargetState::load(target.clone());
        assert!(state.deployments.len() <= MAX_VERSIONS_TO_KEEP);
    }
}
