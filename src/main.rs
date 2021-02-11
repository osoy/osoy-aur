use osoy::operator::{clone, list, remove};
use osoy::{Config, Exec, Location};
use std::str::FromStr;
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
    Install(clone::Opt),
    List(list::Opt),
    Remove(remove::Opt),
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

impl Exec for Opt {
    fn exec(self, config: Config) -> i32 {
        match self {
            Opt::Install(mut opt) => {
                opt.targets = rename_targets(&opt.targets, false);
                opt.exec(config)
            }

            Opt::Remove(mut opt) => {
                opt.targets = rename_targets(&opt.targets, false);
                opt.exec(config)
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
    std::process::exit(Opt::from_args().exec(config))
}
