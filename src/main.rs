#[macro_use]
extern crate osoy;

use osoy::{gitutil, operator, repo, termion, Config, Exec, Location};
use serde::Deserialize;
use std::str::FromStr;
use std::{env, process};
use structopt::clap::AppSettings;
use structopt::StructOpt;
use terminal_size::{terminal_size, Width};

#[derive(StructOpt, Debug)]
#[structopt(
    about = "Search & install packages from the AUR",
    global_settings = &[
        AppSettings::VersionlessSubcommands,
        AppSettings::ColorNever,
    ],
)]
enum Opt {
    #[structopt(alias = "i", about = "Install packages")]
    Install {
        #[structopt(flatten)]
        opt: operator::clone::Opt,
        #[structopt(short, long, help = "Run pacman interactively")]
        interactive: bool,
    },
    #[structopt(about = "List installed packages")]
    List(operator::list::Opt),
    #[structopt(aliases = &["rm", "uninstall"], about = "Uninstall packages")]
    Remove {
        #[structopt(flatten)]
        opt: operator::remove::Opt,
        #[structopt(short, long, help = "Run pacman interactively")]
        interactive: bool,
    },
    #[structopt(alias = "s", about = "Search for packages")]
    Search {
        #[structopt(required = true, min_values = 1, help = Location::about())]
        keywords: Vec<String>,
    },
}

const AUR_URL: &str = "https://aur.archlinux.org/";
const TAB_SIZE: usize = 4;

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AurPackage {
    name: String,
    version: Option<String>,
    description: Option<String>,
    #[serde(rename = "URL")]
    url: Option<String>,
    num_votes: u64,
    popularity: f64,
    out_of_date: Option<u64>,
    maintainer: Option<String>,
    first_submitted: u64,
    last_modified: u64,
}

impl AurPackage {
    fn into_search_entry(self, cols: Option<usize>) -> String {
        let mut description = self
            .description
            .map(|v| format!("\n{}{}", " ".repeat(TAB_SIZE), v))
            .unwrap_or("".into());
        if let Some(cols) = cols {
            let mut line = 0;
            description = description
                .split(' ')
                .fold(" ".repeat(TAB_SIZE), |acc, word| {
                    format!(
                        "{}{}{}",
                        acc,
                        match (acc.len() + word.len() + 1) / cols > line {
                            false => " ".into(),
                            true => {
                                line += 1;
                                format!("\n{}", " ".repeat(TAB_SIZE))
                            }
                        },
                        word
                    )
                });
        }
        [
            self.maintainer
                .map(|v| format!("{}/", v))
                .unwrap_or("".into()),
            self.name,
            self.version.map(|v| format!(" {}", v)).unwrap_or("".into()),
            format!(" [{}]", self.popularity),
            description,
        ]
        .join("")
    }
}

#[derive(Debug, Deserialize)]
struct AurResponse {
    results: Vec<AurPackage>,
}

impl Exec for Opt {
    fn exec(self, config: Config) -> i32 {
        match self {
            Opt::Search { keywords } => {
                let res = match reqwest::blocking::get(&format!(
                    "{}rpc/?v=5&type=search&arg={}",
                    AUR_URL,
                    keywords.join(" ")
                )) {
                    Ok(res) => res,
                    Err(_) => {
                        info!("request failed");
                        return 1;
                    }
                };
                let AurResponse { mut results } = match res.json::<AurResponse>() {
                    Ok(res) => res,
                    Err(_) => {
                        info!("could not parse response");
                        return 1;
                    }
                };
                results.sort_unstable_by_key(|pkg| (pkg.popularity * -1000.0) as i64);
                let cols = terminal_size().map(|(Width(w), _)| w as usize);
                for pkg in results {
                    println!("{}", pkg.into_search_entry(cols));
                }
                0
            }

            Opt::Install {
                mut opt,
                interactive,
            } => {
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
                            let mut args = vec!["-sirc", &name];
                            if !interactive {
                                args.push("--noconfirm");
                            }

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

            Opt::Remove {
                mut opt,
                interactive,
            } => {
                opt.targets = rename_targets(&opt.targets, false);
                let mut errors = 0;

                match repo::iterate_matching_exists(&config.src, opt.targets, opt.regex) {
                    Ok(iter) => {
                        for path in iter {
                            let name = path.file_name().unwrap().to_string_lossy();
                            if opt.force || ask_bool!("remove '{}'?", name) {
                                let mut args = vec!["-Rns", &name];
                                if !interactive {
                                    args.push("--noconfirm");
                                }
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
