use std::env;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub github_token: String,
    pub github_owner: String,
    pub github_repo: String,
    pub github_user: String,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Missing environment variable: {0}")]
    MissingVar(String),
    #[error("Missing environment variable {variable} for profile '{profile}'")]
    MissingProfileVar { variable: String, profile: String },
}

impl Config {
    pub fn from_env(profile: Option<&str>) -> Result<Self, ConfigError> {
        let github_token = read_env("GITHUB_TOKEN", profile)?;
        let github_owner = read_env("GITHUB_OWNER", profile)?;
        let github_repo = read_env("GITHUB_REPO", profile)?;
        let github_user = read_env("GITHUB_USER", profile)?;

        Ok(Config {
            github_token,
            github_owner,
            github_repo,
            github_user,
        })
    }
}

fn read_env(name: &str, profile: Option<&str>) -> Result<String, ConfigError> {
    match profile {
        Some(profile) => {
            let normalized_profile = profile
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() {
                        character.to_ascii_uppercase()
                    } else {
                        '_'
                    }
                })
                .collect::<String>();
            let variable = format!("WORKLOGR_{}_{}", normalized_profile, name);

            env::var(&variable).map_err(|_| ConfigError::MissingProfileVar {
                variable,
                profile: profile.to_string(),
            })
        }
        None => env::var(name).map_err(|_| ConfigError::MissingVar(name.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        values: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvGuard {
        fn set(values: &[(&'static str, Option<&str>)]) -> Self {
            let previous = values
                .iter()
                .map(|(name, _)| (*name, env::var_os(name)))
                .collect();

            for (name, value) in values {
                match value {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
            }

            Self { values: previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in &self.values {
                match value {
                    Some(value) => env::set_var(name, value),
                    None => env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn loads_bare_environment_variables_without_a_profile() {
        let _lock = ENV_LOCK
            .lock()
            .expect("environment lock should be available");
        let _env = EnvGuard::set(&[
            ("GITHUB_TOKEN", Some("bare-token")),
            ("GITHUB_OWNER", Some("bare-owner")),
            ("GITHUB_REPO", Some("bare-repo")),
            ("GITHUB_USER", Some("bare-user")),
        ]);

        let config = Config::from_env(None).expect("bare configuration should load");

        assert_eq!(config.github_token, "bare-token");
        assert_eq!(config.github_owner, "bare-owner");
        assert_eq!(config.github_repo, "bare-repo");
        assert_eq!(config.github_user, "bare-user");
    }

    #[test]
    fn loads_profile_scoped_environment_variables() {
        let _lock = ENV_LOCK
            .lock()
            .expect("environment lock should be available");
        let _env = EnvGuard::set(&[
            ("WORKLOGR_ESTATE_GRID_GITHUB_TOKEN", Some("profile-token")),
            ("WORKLOGR_ESTATE_GRID_GITHUB_OWNER", Some("profile-owner")),
            ("WORKLOGR_ESTATE_GRID_GITHUB_REPO", Some("profile-repo")),
            ("WORKLOGR_ESTATE_GRID_GITHUB_USER", Some("profile-user")),
        ]);

        let config =
            Config::from_env(Some("estate-grid")).expect("profile configuration should load");

        assert_eq!(config.github_token, "profile-token");
        assert_eq!(config.github_owner, "profile-owner");
        assert_eq!(config.github_repo, "profile-repo");
        assert_eq!(config.github_user, "profile-user");
    }

    #[test]
    fn profile_missing_variable_error_names_variable_and_profile() {
        let _lock = ENV_LOCK
            .lock()
            .expect("environment lock should be available");
        let _env = EnvGuard::set(&[
            ("WORKLOGR_NSFAS_GITHUB_TOKEN", Some("profile-token")),
            ("WORKLOGR_NSFAS_GITHUB_OWNER", Some("profile-owner")),
            ("WORKLOGR_NSFAS_GITHUB_REPO", None),
            ("WORKLOGR_NSFAS_GITHUB_USER", Some("profile-user")),
        ]);

        let error = Config::from_env(Some("nsfas")).expect_err("missing repo should fail");

        assert_eq!(
            error.to_string(),
            "Missing environment variable WORKLOGR_NSFAS_GITHUB_REPO for profile 'nsfas'"
        );
    }
}
