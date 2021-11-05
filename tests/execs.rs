use std::{env, fs, path::PathBuf};

use duct::cmd;
use nix::sys::stat::{stat, Mode};

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");

const CLIENT_BIN: &str = env!("CARGO_BIN_EXE_client");
const SERVER_BIN: &str = env!("CARGO_BIN_EXE_server");

#[test]
fn run_execs() {
    let all_tests_succeed = fs::read_dir(DIR)
        .expect("read test directory")
        .filter_map(|readdir| {
            let entry = readdir.expect("read entry");
            if Mode::from_bits_truncate(stat(&entry.path()).expect("stat file").st_mode)
                .intersects(Mode::S_IXUSR | Mode::S_IXGRP | Mode::S_IXOTH)
            {
                Some(entry.path())
            } else {
                None
            }
        })
        .map(|exec| {
            print!("test {:?} ... ", exec);

            let path = env::join_paths(
                env::split_paths(&env::var_os("PATH").unwrap())
                    .chain([PathBuf::from(DIR)].iter().cloned())
                    .chain(
                        [PathBuf::from(CLIENT_BIN), PathBuf::from(SERVER_BIN)]
                            .iter()
                            .cloned()
                            .map(|mut dir| {
                                dir.pop();
                                dir
                            }),
                    ),
            )
            .expect("valid PATH's element");

            cmd!(exec).dir(DIR).env("PATH", path).run()
        })
        .map(|ret| match ret {
            Ok(_) => {
                println!("ok");
                true
            }
            Err(err) => {
                println!("{}", err);
                false
            }
        })
        .all(|is_success| is_success);

    if !all_tests_succeed {
        panic!("some test failed")
    }
}
