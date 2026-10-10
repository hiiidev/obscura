//! Backend policy is independent of driver loading so every fallback is testable.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Auto,
    Hardware,
    Software,
}

impl Mode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "auto" => Ok(Self::Auto),
            "hardware" => Ok(Self::Hardware),
            "software" => Ok(Self::Software),
            _ => Err(format!(
                "invalid WebGL backend selection {value:?}; use auto, hardware or software"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backend {
    Metal,
    Vulkan,
    SwiftShader,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    MacOS,
    Linux,
    Unsupported,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOS
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Unsupported
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Attempt {
    pub backend: Backend,
    pub reason: String,
}

/// An initializer must create a usable context, not only initialize a display.
/// Thus device/configuration/context failures all participate in fallback.
pub fn select<T>(
    platform: Platform,
    mode: Mode,
    fail_if_caveat: bool,
    mut initialize: impl FnMut(Backend) -> Result<T, String>,
) -> Result<(T, Backend, Vec<Attempt>), Vec<Attempt>> {
    let choices: &[Backend] = match (platform, mode) {
        (Platform::MacOS, Mode::Auto | Mode::Hardware) => &[Backend::Metal],
        (Platform::Linux, Mode::Auto) => &[Backend::Vulkan, Backend::SwiftShader],
        (Platform::Linux, Mode::Hardware) => &[Backend::Vulkan],
        (Platform::Linux, Mode::Software) => &[Backend::SwiftShader],
        _ => &[],
    };
    let mut attempts = Vec::new();
    for &backend in choices {
        if backend == Backend::SwiftShader && fail_if_caveat {
            attempts.push(Attempt {
                backend,
                reason: "software rendering excluded by failIfMajorPerformanceCaveat".into(),
            });
            continue;
        }
        match initialize(backend) {
            Ok(context) => return Ok((context, backend, attempts)),
            Err(reason) => attempts.push(Attempt { backend, reason }),
        }
    }
    Err(attempts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_values_are_explicit_and_invalid_values_are_not_auto() {
        for (input, expected) in [
            ("auto", Mode::Auto),
            ("hardware", Mode::Hardware),
            ("software", Mode::Software),
        ] {
            assert_eq!(Mode::parse(input).unwrap(), expected);
        }
        for input in ["", "gpu", "AUTO", " auto"] {
            assert!(Mode::parse(input).is_err());
        }
    }
    #[test]
    fn linux_uses_hardware_without_creating_software_resources() {
        let mut calls = vec![];
        let (value, backend, failures) = select(Platform::Linux, Mode::Auto, false, |b| {
            calls.push(b);
            Ok(7)
        })
        .unwrap();
        assert_eq!(
            (value, backend, calls),
            (7, Backend::Vulkan, vec![Backend::Vulkan])
        );
        assert!(failures.is_empty());
    }
    #[test]
    fn every_hardware_initialization_failure_can_fall_back() {
        for reason in [
            "missing driver",
            "no device",
            "no config",
            "context creation failed",
            "make current failed",
            "allocation failed",
        ] {
            let ((), b, failures) = select(Platform::Linux, Mode::Auto, false, |b| {
                if b == Backend::Vulkan {
                    Err(reason.to_string())
                } else {
                    Ok(())
                }
            })
            .unwrap();
            assert_eq!(b, Backend::SwiftShader);
            assert_eq!(failures.len(), 1);
            assert_eq!(failures[0].reason, reason);
        }
    }
    #[test]
    fn forced_modes_do_not_try_the_other_backend() {
        for (mode, expected) in [
            (Mode::Hardware, Backend::Vulkan),
            (Mode::Software, Backend::SwiftShader),
        ] {
            let mut calls = vec![];
            let failure = select::<()>(Platform::Linux, mode, false, |b| {
                calls.push(b);
                Err("unavailable".into())
            })
            .unwrap_err();
            assert_eq!(calls, vec![expected]);
            assert_eq!(failure.len(), 1);
        }
    }
    #[test]
    fn exhausted_fallback_preserves_both_reasons() {
        let failures = select::<()>(Platform::Linux, Mode::Auto, false, |b| {
            Err(format!("{b:?} failed"))
        })
        .unwrap_err();
        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].backend, Backend::Vulkan);
        assert_eq!(failures[1].backend, Backend::SwiftShader);
    }
    #[test]
    fn caveat_flag_prevents_software_initialization() {
        let mut calls = vec![];
        let failures = select::<()>(Platform::Linux, Mode::Auto, true, |b| {
            calls.push(b);
            Err("no GPU".into())
        })
        .unwrap_err();
        assert_eq!(calls, vec![Backend::Vulkan]);
        assert_eq!(failures.len(), 2);
        assert!(select(Platform::Linux, Mode::Software, true, |_| Ok(())).is_err());
    }
    #[test]
    fn mac_uses_metal_and_unsupported_platforms_do_not_load_drivers() {
        let (_, backend, _) = select(Platform::MacOS, Mode::Auto, false, |b| Ok(b)).unwrap();
        assert_eq!(backend, Backend::Metal);
        for (platform, mode) in [
            (Platform::Unsupported, Mode::Auto),
            (Platform::MacOS, Mode::Software),
        ] {
            assert!(
                select::<()>(platform, mode, false, |_| panic!("must not initialize"))
                    .unwrap_err()
                    .is_empty()
            );
        }
    }
}
