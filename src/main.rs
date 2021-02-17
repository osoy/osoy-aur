#[macro_use]
extern crate osoy;

use osoy::{gitutil, operator, repo, termion, Config, Exec, Location};
use std::str::FromStr;
use std::{env, process};
use structopt::clap::AppSettings;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(
    about = "Search & install packages from the AUR",
    global_settings = &[
        AppSettings::VersionlessSubcommands,
        AppSettings::ColorNever,
    ],
)]
enum Opt {
    Install(operator::clone::Opt),
    List(operator::list::Opt),
    Remove(operator::remove::Opt),
}

const AUR_URL: &str = "https://aur.archlinux.org/";

fn rename_targets(targets: &[Location], fill_empty: bool) -> Vec<Location> {
    match !fill_empty || targets.len() > 0 {
        true => targets
            .iter()
            .map(|target| Location::from_str(&format!("{}{}", AUR_URL, target)).unwrap())
            .collect(),
        false => vec![Location::from_str(AUR_URL).unwrap()],
    }
}

fn force_remove_dir(path: &str) -> i32 {
    process::Command::new("rm")
        .args(&["-rf", path])
        .status()
        .ok()
        .map(|status| status.code())
        .flatten()
        .map(|code| code)
        .unwrap_or(1)
}

impl Exec for Opt {
    fn exec(self, config: Config) -> i32 {
        match self {
            Opt::Install(mut opt) => {
                opt.targets = rename_targets(&opt.targets, false);
                let auth_cache = gitutil::AuthCache::new();
                let mut errors = 0;
                let mut paths = vec![];

                for location in opt.targets {
                    let id = location.id();
                    let path = config.src.join(&id);

                    if path.exists() {
                        paths.push(path);
                    } else {
                        match gitutil::clone(&path, &id, &location.url(), &auth_cache) {
                            Ok(_) => {
                                paths.push(path);
                                gitutil::log("done", id);
                            }
                            Err(err) => {
                                errors += 1;
                                gitutil::log("failed", id);
                                if opt.verbose {
                                    gitutil::log("", err);
                                }
                                force_remove_dir(&path.to_string_lossy());
                            }
                        }
                    }
                }

                info!("installing...");

                for path in paths {
                    match env::set_current_dir(&path) {
                        Ok(_) => {
                            let name = path.file_name().unwrap().to_string_lossy();
                            let cmd = "makepkg";
                            let args = ["-sirc", "--noconfirm", &name];

                            if opt.verbose {
                                println!("> {} {}", cmd, args.join(" "));
                            }

                            let exit_code = process::Command::new(cmd)
                                .args(&args)
                                .stdin(process::Stdio::inherit())
                                .stderr(process::Stdio::inherit())
                                .stdout(process::Stdio::inherit())
                                .env("PWD", path.display().to_string())
                                .status()
                                .ok()
                                .map(|status| status.code())
                                .flatten()
                                .map(|code| code)
                                .unwrap_or(1);

                            if exit_code != 0 {
                                errors += 1;
                                info!("failed to install '{}' [{}]", name, exit_code)
                            }
                        }
                        Err(err) => {
                            errors += 1;
                            info!("could not access '{}': {}", path.display(), err)
                        }
                    }
                }

                errors
            }

            Opt::Remove(mut opt) => {
                opt.targets = rename_targets(&opt.targets, false);
                let mut errors = 0;

                match repo::iterate_matching_exists(&config.src, opt.targets, opt.regex) {
                    Ok(iter) => {
                        for path in iter {
                            let name = path.file_name().unwrap().to_string_lossy();
                            if opt.force || ask_bool!("remove '{}'?", name) {
                                let mut args = vec!["-Rns", "--noconfirm", &name];
                                let mut cmd = "pacman";

                                if env::var("USER").map(|user| user != "root").unwrap_or(true) {
                                    args.insert(0, cmd);
                                    cmd = "sudo";
                                }

                                if opt.verbose {
                                    println!("> {} {}", cmd, args.join(" "));
                                }

                                let exit_code = process::Command::new(cmd)
                                    .args(&args)
                                    .stdin(process::Stdio::inherit())
                                    .stderr(process::Stdio::inherit())
                                    .stdout(process::Stdio::inherit())
                                    .status()
                                    .ok()
                                    .map(|status| status.code())
                                    .flatten()
                                    .map(|code| code)
                                    .unwrap_or(1);

                                if exit_code != 0 {
                                    errors += 1;
                                    info!("failed to uninstall {}", name);
                                }

                                if exit_code == 0 || opt.force {
                                    let exit_code = force_remove_dir(&path.to_string_lossy());
                                    if exit_code != 0 {
                                        errors += 1;
                                        info!(
                                            "failed to remove '{}' [{}]",
                                            path.display(),
                                            exit_code
                                        )
                                    } else {
                                        if opt.verbose {
                                            info!("removed '{}'", name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        errors += 1;
                        info!("{}", err)
                    }
                }

                errors
            }

            Opt::List(mut opt) => {
                opt.targets = rename_targets(&opt.targets, true);
                opt.exec(config)
            }
        }
    }
}

fn main() {
    let mut config = Config::from_env();
    config.src.pop();
    config.src.push("aur");
    process::exit(Opt::from_args().exec(config))
}
