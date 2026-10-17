use pkgx_core::{
    command::{CommandOutput, CommandRunner, CommandSpec},
    recipe::Recipe,
    Error,
};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Mutex},
    time::Duration,
};
const RECIPE: &str = include_str!("../../../tests/fixtures/recipe.toml");
struct Mock {
    tampered: bool,
    artifact: Mutex<Option<PathBuf>>,
    calls: Mutex<Vec<String>>,
}
impl CommandRunner for Mock {
    fn run(&self, spec: &CommandSpec, _: Duration, _: &AtomicBool) -> Result<CommandOutput, Error> {
        self.calls.lock().unwrap().push(spec.program.clone());
        let text: String = match spec.program.as_str() {
            "curl" => {
                assert_eq!(spec.args[0], "--disable");
                assert!(spec.args.contains(&"=https".into()));
                assert!(spec.args.contains(&"--max-filesize".into()));
                let output = spec.args.iter().position(|arg| arg == "--output").unwrap();
                assert_eq!(spec.args[output + 1], "-");
                "abc".into()
            }
            "sha256sum" => {
                let path = PathBuf::from(spec.args.last().unwrap());
                assert_eq!(std::fs::read(&path).unwrap(), b"abc");
                *self.artifact.lock().unwrap() = Some(path);
                if self.tampered {
                    "1111111111111111111111111111111111111111111111111111111111111111  artifact\n"
                        .into()
                } else {
                    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  artifact\n"
                        .into()
                }
            }
            _ => panic!(
                "recipe preparation must never invoke an interpreter: {}",
                spec.program
            ),
        };
        Ok(CommandOutput {
            status: Some(0),
            stdout: text.into_bytes(),
            stderr: vec![],
        })
    }
}
#[test]
fn verified_download_is_bounded_discarded_and_never_executed() {
    let recipe = Recipe::parse(RECIPE).unwrap();
    let runner = Mock {
        tampered: false,
        artifact: Mutex::new(None),
        calls: Mutex::new(vec![]),
    };
    let result = recipe.verify_download(&runner, "linux").unwrap();
    assert!(result.verified);
    assert_eq!(result.byte_count, 3);
    assert!(!result.recipe.execution_supported);
    assert!(!runner.artifact.lock().unwrap().as_ref().unwrap().exists());
    assert_eq!(*runner.calls.lock().unwrap(), ["curl", "sha256sum"]);
}
#[test]
fn tampered_download_is_rejected_and_discarded() {
    let runner = Mock {
        tampered: true,
        artifact: Mutex::new(None),
        calls: Mutex::new(vec![]),
    };
    assert!(matches!(
        Recipe::parse(RECIPE)
            .unwrap()
            .verify_download(&runner, "linux"),
        Err(Error::Safety(_))
    ));
    assert!(!runner.artifact.lock().unwrap().as_ref().unwrap().exists());
}
#[test]
fn unpinned_insecure_or_interpolated_recipes_are_rejected() {
    for input in [
        RECIPE.replace("version = 1\n", "version = 2\n"),
        RECIPE.replace("https://example.org", "http://example.org"),
        RECIPE.replace("https://example.org", "https://user:secret@example.org"),
        RECIPE.replace("install.sh\"", "install.sh?token=secret\""),
        RECIPE.replace("1.2.3", "latest"),
        RECIPE.replace(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "missing",
        ),
        RECIPE.replace("/opt/example-tool/bin/example-tool", "/opt/../etc/passwd"),
        RECIPE.replace("args = [\"--prefix\"", "args = [\"$(id)\""),
        format!("{RECIPE}\nunknown = true"),
    ] {
        assert!(Recipe::parse(&input).is_err());
    }
}
