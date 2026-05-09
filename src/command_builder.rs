pub const DEFAULT_FC_BIN: &str = "firecracker";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CommandStdio {
    #[default]
    Inherit,
    Null,
    Path(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VMCommand {
    pub bin: String,
    pub args: Vec<String>,
    pub stdin: CommandStdio,
    pub stdout: CommandStdio,
    pub stderr: CommandStdio,
}

impl VMCommand {
    pub fn argv(&self) -> Vec<String> {
        let mut argv = vec![self.bin.clone()];
        argv.extend(self.args.clone());
        argv
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VMCommandBuilder {
    bin: Option<String>,
    args: Vec<String>,
    socket_path: Option<String>,
    stdin: CommandStdio,
    stdout: CommandStdio,
    stderr: CommandStdio,
}

impl VMCommandBuilder {
    pub fn args(&self) -> Option<&[String]> {
        (!self.args.is_empty()).then_some(self.args.as_slice())
    }

    pub fn with_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn add_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn bin(&self) -> &str {
        self.bin.as_deref().unwrap_or(DEFAULT_FC_BIN)
    }

    pub fn with_bin(mut self, bin: impl Into<String>) -> Self {
        self.bin = Some(bin.into());
        self
    }

    pub fn socket_path_args(&self) -> Option<Vec<String>> {
        self.socket_path
            .as_ref()
            .map(|socket_path| vec!["--api-sock".to_string(), socket_path.clone()])
    }

    pub fn with_socket_path(mut self, path: impl Into<String>) -> Self {
        self.socket_path = Some(path.into());
        self
    }

    pub fn with_stdin(mut self, stdin: CommandStdio) -> Self {
        self.stdin = stdin;
        self
    }

    pub fn with_stdin_path(mut self, path: impl Into<String>) -> Self {
        self.stdin = CommandStdio::Path(path.into());
        self
    }

    pub fn with_stdout(mut self, stdout: CommandStdio) -> Self {
        self.stdout = stdout;
        self
    }

    pub fn with_stdout_path(mut self, path: impl Into<String>) -> Self {
        self.stdout = CommandStdio::Path(path.into());
        self
    }

    pub fn with_stderr(mut self, stderr: CommandStdio) -> Self {
        self.stderr = stderr;
        self
    }

    pub fn with_stderr_path(mut self, path: impl Into<String>) -> Self {
        self.stderr = CommandStdio::Path(path.into());
        self
    }

    pub fn build(&self) -> VMCommand {
        let mut args = Vec::new();
        if let Some(socket_path) = self.socket_path_args() {
            args.extend(socket_path);
        }
        args.extend(self.args.clone());

        VMCommand {
            bin: self.bin().to_string(),
            args,
            stdin: self.stdin.clone(),
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
        }
    }
}

pub fn seccomp_args(enabled: bool, filter: Option<&str>) -> Vec<String> {
    match (enabled, filter) {
        (false, _) => vec!["--no-seccomp".to_string()],
        (true, Some(filter)) if !filter.is_empty() => {
            vec!["--seccomp-filter".to_string(), filter.to_string()]
        }
        _ => Vec::new(),
    }
}
