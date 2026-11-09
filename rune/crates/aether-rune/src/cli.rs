//! Host flags are parsed only after Rune has removed its own headless flags.
use std::path::PathBuf;
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub profile: Option<PathBuf>,
    pub import_profile: Option<PathBuf>,
    pub smoke: bool,
    pub two_windows: bool,
}
impl Options {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--profile" | "--import-profile" => {
                    let flag = args[index].clone();
                    index += 1;
                    let value = args
                        .get(index)
                        .filter(|value| !value.starts_with("--"))
                        .ok_or_else(|| format!("{flag} requires an absolute path"))?;
                    let target = if flag == "--profile" {
                        &mut options.profile
                    } else {
                        &mut options.import_profile
                    };
                    if target.is_some() {
                        return Err(format!("{flag} may only be supplied once"));
                    }
                    *target = Some(PathBuf::from(value));
                }
                "--smoke" => options.smoke = true,
                "--two-windows" => options.two_windows = true,
                flag => return Err(format!("Unknown application argument: {flag}")),
            }
            index += 1;
        }
        if options.smoke && (options.profile.is_some() || options.import_profile.is_some()) {
            return Err(
                "--smoke always uses a new temporary profile; omit profile/import flags".into(),
            );
        }
        Ok(options)
    }
}
