//! The player's launch options for a game (its Manage tab), read the way Steam reads them.
//!
//! Text without `%command%` is extra arguments, added after everything else, as it always was:
//! `--res=1080` stays one argument, whatever `=` it has. With `%command%`, the words before it set
//! variables for the game (`NAME=value`) and the words after it are its arguments:
//! `ICO_ISO="D:\My Games\ico.iso" %command% --fullscreen`.

/// What the launch options give the game: variables to set, and arguments to add.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub env: Vec<(String, String)>,
    pub args: Vec<String>,
}

/// Where the game's command goes in the launch options.
pub const COMMAND: &str = "%command%";

/// The player's launch options, or why they can't be used, in words the player can act on.
pub fn parse(text: &str) -> Result<Options, String> {
    let words = crate::split_args(text);
    let at = words.iter().position(|w| w == COMMAND);
    let Some(at) = at else {
        return Ok(Options { env: vec![], args: crate::join_paths(words) });
    };
    if words[at + 1..].iter().any(|w| w == COMMAND) {
        return Err(format!("Put {COMMAND} in your launch options only once."));
    }
    let mut env = vec![];
    for word in &words[..at] {
        env.push(variable(word)?);
    }
    let args = crate::join_paths(words[at + 1..].to_vec());
    Ok(Options { env, args })
}

/// One `NAME=value` before `%command%`.
fn variable(word: &str) -> Result<(String, String), String> {
    let Some((name, value)) = word.split_once('=') else {
        return Err(format!("Before {COMMAND}, launch options can only set variables, like NAME=value. \"{word}\" isn't one."));
    };
    if !crate::profile::env_name_shape(name) {
        return Err(format!("\"{name}\" can't be a variable's name: use letters, digits and _, and don't start with a digit."));
    }
    // The launcher's own variables tell the game how it was started; a game must be able to trust them.
    if name.to_ascii_uppercase().starts_with("RECOMPLAUNCHER") {
        return Err(format!("{name} can't be set in launch options: names starting with RECOMPLAUNCHER are the launcher's own."));
    }
    Ok((name.to_string(), value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Options {
        parse(text).unwrap()
    }

    #[test]
    fn text_without_command_is_arguments_as_before() {
        assert_eq!(ok("--fullscreen -v"), Options { env: vec![], args: vec!["--fullscreen".into(), "-v".into()] });
    }

    #[test]
    fn an_equals_sign_without_command_stays_an_argument() {
        assert_eq!(ok("--res=1080 NAME=value"), Options { env: vec![], args: vec!["--res=1080".into(), "NAME=value".into()] });
    }

    #[test]
    fn empty_options_give_nothing() {
        assert_eq!(ok("   "), Options::default());
    }

    #[test]
    fn variables_before_command_and_arguments_after() {
        let o = ok(r#"ICO_ISO="D:\My Games\ico.iso" PROTON_LOG=1 %command% --fullscreen"#);
        assert_eq!(o.env, vec![("ICO_ISO".to_string(), r"D:\My Games\ico.iso".to_string()), ("PROTON_LOG".to_string(), "1".to_string())]);
        assert_eq!(o.args, vec!["--fullscreen".to_string()]);
    }

    #[test]
    fn command_alone_gives_nothing() {
        assert_eq!(ok("%command%"), Options::default());
    }

    #[test]
    fn a_variable_can_be_empty() {
        assert_eq!(ok("LD_PRELOAD= %command%").env, vec![("LD_PRELOAD".to_string(), String::new())]);
    }

    #[test]
    fn a_value_keeps_its_own_equals_signs() {
        assert_eq!(ok("DXVK_CONFIG=dxgi.syncInterval=1 %command%").env, vec![("DXVK_CONFIG".to_string(), "dxgi.syncInterval=1".to_string())]);
    }

    #[test]
    fn a_word_before_command_has_to_be_a_variable() {
        assert!(parse("--fullscreen %command%").unwrap_err().contains("\"--fullscreen\" isn't one"));
    }

    #[test]
    fn a_name_has_to_look_like_a_name() {
        assert!(parse("1ICO=x %command%").is_err());
        assert!(parse("=x %command%").is_err());
    }

    #[test]
    fn the_launchers_own_names_are_refused() {
        assert!(parse("RECOMPLAUNCHER_MODE=desktop %command%").unwrap_err().contains("launcher's own"));
        assert!(parse("recomplauncher=1 %command%").is_err());
    }

    #[test]
    fn command_only_once() {
        assert!(parse("A=1 %command% %command%").unwrap_err().contains("only once"));
    }
}
