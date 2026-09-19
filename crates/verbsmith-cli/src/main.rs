mod output;
mod sync;
mod tui;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    env, fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use verbsmith_core::{
    ExecutionOptions, Header, HttpRequest, RunResult, Workspace, WorkspaceManifest,
    evaluate_assertions, execute, extract_captures, format_document,
};

#[derive(Debug, Parser)]
#[command(name = "verbsmith", version, about, long_about = None)]
struct Cli {
    /// Increase diagnostic logging. Secrets remain redacted.
    #[arg(long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Open the interactive terminal workspace.
    Tui {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Send a one-off request.
    Request {
        /// HTTP method, such as GET or POST.
        method: String,
        /// Absolute request URL.
        url: String,
        #[arg(short = 'H', long = "header")]
        headers: Vec<String>,
        #[arg(short = 'd', long = "data")]
        data: Option<String>,
        #[arg(long = "var", value_parser = parse_pair)]
        variables: Vec<(String, String)>,
        #[arg(long)]
        no_follow: bool,
        #[arg(short = 'k', long)]
        insecure: bool,
        #[arg(long, default_value_t = 30)]
        timeout: u64,
        #[arg(long)]
        proxy: Option<String>,
        #[arg(long, default_value_t = 64)]
        max_response_mib: usize,
        /// Retry idempotent requests after transport errors, 408, 429, and 5xx responses.
        #[arg(long, default_value_t = 0)]
        retry: u32,
        /// Permit retries for non-idempotent methods such as POST.
        #[arg(long)]
        retry_all: bool,
        #[arg(long, value_enum, default_value_t = FormatArg::Pretty)]
        format: FormatArg,
    },
    /// Run saved requests and their dependencies.
    Run(RunArgs),
    /// Run saved assertions and fail if any assertion fails.
    Test(RunArgs),
    /// Create a new local-first workspace.
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    /// Parse and validate every request in a workspace.
    Lint {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Apply the canonical request-file formatter.
    Fmt {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        check: bool,
    },
    /// Import another request representation.
    Import {
        #[command(subcommand)]
        source: ImportCommand,
    },
    /// Store secrets in the operating-system credential manager.
    Vault {
        #[command(subcommand)]
        command: VaultCommand,
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
    },
    /// Synchronize encrypted workspace revisions with a configured server.
    Sync {
        #[command(subcommand)]
        command: SyncCommand,
        #[arg(long, default_value = ".")]
        workspace: PathBuf,
    },
    /// Inspect this installation and its transport capabilities.
    Doctor,
    /// Generate shell completion code.
    Completion { shell: clap_complete::Shell },
    /// Generate a manual page.
    Man {
        #[arg(default_value = "verbsmith.1")]
        output: PathBuf,
    },
}

#[derive(Debug, clap::Args)]
struct RunArgs {
    /// Request name, tag prefixed by `tag:`, file, or `all`.
    #[arg(default_value = "all")]
    selector: String,
    #[arg(long)]
    env: Option<String>,
    #[arg(long = "var", value_parser = parse_pair)]
    variables: Vec<(String, String)>,
    #[arg(long, value_enum, default_value_t = FormatArg::Pretty)]
    format: FormatArg,
    #[arg(long)]
    fail_fast: bool,
    #[arg(long)]
    insecure: bool,
    #[arg(long)]
    proxy: Option<String>,
    #[arg(long, default_value_t = 30)]
    timeout: u64,
    #[arg(long, default_value_t = 64)]
    max_response_mib: usize,
    #[arg(long, default_value_t = 0)]
    retry: u32,
    #[arg(long)]
    retry_all: bool,
    #[arg(long, default_value = ".")]
    workspace: PathBuf,
}

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
enum FormatArg {
    #[default]
    Pretty,
    Raw,
    Json,
    Jsonl,
    Junit,
    Sarif,
}

#[derive(Debug, Subcommand)]
enum ImportCommand {
    /// Convert a curl command into a Verbsmith request file.
    Curl {
        /// Curl command, quoted as a single argument.
        command: String,
        #[arg(short, long, default_value = "requests/imported.http")]
        output: PathBuf,
    },
    /// Convert a Postman Collection v2.1 JSON document.
    Postman {
        input: PathBuf,
        #[arg(short, long, default_value = "requests/imported-postman")]
        output: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
enum VaultCommand {
    /// Store or replace a secret.
    Set { name: String },
    /// Confirm that a secret exists without revealing it.
    Get { name: String },
    /// Remove a secret.
    Remove { name: String },
    /// List secret names. Values are never displayed.
    List,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SyncCommand {
    /// Encrypt and upload the current workspace revision.
    Push,
    /// Download and decrypt the current server revision.
    Pull {
        /// Replace locally modified files after creating a backup.
        #[arg(long)]
        force: bool,
    },
    /// Compare the local and remote revision identifiers.
    Status,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let filter = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| filter.into()),
        )
        .with_writer(io::stderr)
        .init();

    match dispatch(cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(classify_error(&error))
        }
    }
}

fn dispatch(cli: Cli) -> Result<u8> {
    let command = match cli.command {
        Some(command) => command,
        None if io::stdin().is_terminal() && io::stdout().is_terminal() => {
            Command::Tui { path: ".".into() }
        }
        None => {
            Cli::command().print_help()?;
            println!();
            return Ok(0);
        }
    };

    match command {
        Command::Tui { path } => {
            let workspace = Workspace::discover(path)?;
            tui::run(workspace)?;
            Ok(0)
        }
        Command::Request {
            method,
            url,
            headers,
            data,
            variables,
            no_follow,
            insecure,
            timeout,
            proxy,
            max_response_mib,
            retry,
            retry_all,
            format,
        } => {
            let request = one_off_request(method, url, headers, data)?;
            let variables = variables.into_iter().collect::<BTreeMap<_, _>>();
            let options = ExecutionOptions {
                follow_redirects: !no_follow,
                insecure,
                timeout: Duration::from_secs(timeout),
                proxy,
                variables,
                max_response_bytes: max_response_mib.saturating_mul(1024 * 1024),
                retries: retry,
                retry_non_idempotent: retry_all,
            };
            let response = execute(&request, &options)?;
            let result = RunResult {
                assertions: evaluate_assertions(&request, &response),
                captures: extract_captures(&request, &response),
                request,
                response,
            };
            output::render(&[result], format, &[])?;
            Ok(0)
        }
        Command::Run(args) => run_workspace(args, false),
        Command::Test(args) => run_workspace(args, true),
        Command::Init { path, name } => {
            init_workspace(&path, name.as_deref())?;
            println!(
                "Initialized Verbsmith workspace in {}",
                path.canonicalize()?.display()
            );
            Ok(0)
        }
        Command::Lint { path } => {
            let workspace = Workspace::discover(path)?;
            let requests = workspace.requests()?;
            validate_dependencies(&requests)?;
            println!(
                "Checked {} requests in {}",
                requests.len(),
                workspace.root.display()
            );
            Ok(0)
        }
        Command::Fmt { path, check } => format_workspace(&path, check),
        Command::Import { source } => import(source),
        Command::Vault { command, workspace } => vault(command, &workspace),
        Command::Sync { command, workspace } => {
            let workspace = Workspace::discover(workspace)?;
            sync::run(command, &workspace)
        }
        Command::Doctor => doctor(),
        Command::Completion { shell } => {
            clap_complete::generate(shell, &mut Cli::command(), "verbsmith", &mut io::stdout());
            Ok(0)
        }
        Command::Man { output } => {
            let manual = clap_mangen::Man::new(Cli::command());
            let mut buffer = Vec::new();
            manual.render(&mut buffer)?;
            fs::write(&output, buffer).with_context(|| format!("writing {}", output.display()))?;
            println!("Wrote {}", output.display());
            Ok(0)
        }
    }
}

fn one_off_request(
    method: String,
    url: String,
    headers: Vec<String>,
    data: Option<String>,
) -> Result<HttpRequest> {
    let headers = headers
        .into_iter()
        .map(|header| {
            let (name, value) = header
                .split_once(':')
                .ok_or_else(|| anyhow!("header must be NAME: VALUE"))?;
            Ok(Header {
                name: name.trim().into(),
                value: value.trim().into(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(HttpRequest {
        name: "ad-hoc".into(),
        method: method.to_ascii_uppercase(),
        url,
        headers,
        body: data.unwrap_or_default(),
        source: PathBuf::from("<command-line>"),
        line: 1,
        timeout: None,
        tags: Vec::new(),
        depends_on: Vec::new(),
        assertions: Vec::new(),
        captures: Vec::new(),
        disabled: false,
    })
}

fn run_workspace(args: RunArgs, test_mode: bool) -> Result<u8> {
    let workspace = Workspace::discover(&args.workspace)?;
    let environment = workspace.environment(args.env.as_deref())?;
    if !environment.secrets.is_empty() {
        eprintln!(
            "warning: plaintext [secrets] are supported only for migration; use the vault before committing this file"
        );
    }
    let mut variables = workspace.variables(&environment);
    variables.extend(args.variables);
    let mut redactions = environment.secrets.values().cloned().collect::<Vec<_>>();
    redactions.extend(resolve_secret_references(&workspace, &mut variables)?);
    let requests = workspace.requests()?;
    validate_dependencies(&requests)?;
    let selected = select_requests(&requests, &args.selector)?;
    let ordered = dependency_order(&requests, &selected)?;
    let mut results = Vec::new();
    let mut failed = false;

    for request in ordered {
        if request.disabled {
            continue;
        }
        let options = ExecutionOptions {
            follow_redirects: true,
            insecure: args.insecure,
            timeout: Duration::from_secs(args.timeout),
            proxy: args.proxy.clone(),
            variables: variables.clone(),
            max_response_bytes: args.max_response_mib.saturating_mul(1024 * 1024),
            retries: args.retry,
            retry_non_idempotent: args.retry_all,
        };
        let response = execute(request, &options)?;
        let assertions = evaluate_assertions(request, &response);
        let captures = extract_captures(request, &response);
        variables.extend(captures.clone());
        let result = RunResult {
            request: request.clone(),
            response,
            assertions,
            captures,
        };
        failed |= !result.passed();
        results.push(result);
        if failed && args.fail_fast {
            break;
        }
    }
    output::render(&results, args.format, &redactions)?;
    Ok(if test_mode && failed { 1 } else { 0 })
}

fn select_requests<'a>(
    requests: &'a [HttpRequest],
    selector: &str,
) -> Result<Vec<&'a HttpRequest>> {
    let selected: Vec<&HttpRequest> = if selector == "all" {
        requests.iter().collect()
    } else if let Some(tag) = selector.strip_prefix("tag:") {
        requests
            .iter()
            .filter(|request| request.tags.iter().any(|candidate| candidate == tag))
            .collect()
    } else {
        requests
            .iter()
            .filter(|request| request.name == selector || request.source.ends_with(selector))
            .collect()
    };
    if selected.is_empty() {
        bail!("no requests matched `{selector}`");
    }
    Ok(selected)
}

fn validate_dependencies(requests: &[HttpRequest]) -> Result<()> {
    let names = requests
        .iter()
        .map(|request| request.name.as_str())
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    for request in requests {
        if !seen.insert(&request.name) {
            bail!("duplicate request name `{}`", request.name);
        }
        for dependency in &request.depends_on {
            if !names.contains(dependency.as_str()) {
                bail!(
                    "request `{}` depends on unknown request `{dependency}`",
                    request.name
                );
            }
        }
    }
    let selected = requests.iter().collect::<Vec<_>>();
    dependency_order(requests, &selected)?;
    Ok(())
}

fn dependency_order<'a>(
    all: &'a [HttpRequest],
    selected: &[&'a HttpRequest],
) -> Result<Vec<&'a HttpRequest>> {
    let by_name = all
        .iter()
        .map(|request| (request.name.as_str(), request))
        .collect::<HashMap<_, _>>();
    let mut ordered = Vec::new();
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();

    fn visit<'a>(
        request: &'a HttpRequest,
        by_name: &HashMap<&str, &'a HttpRequest>,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
        ordered: &mut Vec<&'a HttpRequest>,
    ) -> Result<()> {
        if visited.contains(&request.name) {
            return Ok(());
        }
        if !visiting.insert(request.name.clone()) {
            bail!("dependency cycle contains `{}`", request.name);
        }
        for dependency in &request.depends_on {
            visit(
                by_name[dependency.as_str()],
                by_name,
                visiting,
                visited,
                ordered,
            )?;
        }
        visiting.remove(&request.name);
        visited.insert(request.name.clone());
        ordered.push(request);
        Ok(())
    }

    for request in selected {
        visit(request, &by_name, &mut visiting, &mut visited, &mut ordered)?;
    }
    Ok(ordered)
}

fn init_workspace(path: &Path, name: Option<&str>) -> Result<()> {
    fs::create_dir_all(path.join("requests"))?;
    fs::create_dir_all(path.join("environments"))?;
    let manifest_path = path.join("verbsmith.toml");
    if manifest_path.exists() {
        bail!("{} already exists", manifest_path.display());
    }
    let workspace_name = name
        .map(str::to_owned)
        .or_else(|| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "verbsmith-workspace".into());
    let manifest = WorkspaceManifest {
        name: workspace_name,
        ..WorkspaceManifest::default()
    };
    fs::write(&manifest_path, toml::to_string_pretty(&manifest)?)?;
    fs::write(
        path.join("requests/example.http"),
        "# @name health\n# @assert status == 200\nGET {{base_url}}/health\nAccept: application/json\n",
    )?;
    fs::write(
        path.join("environments/local.toml"),
        "[values]\nbase_url = \"http://localhost:3000\"\n",
    )?;
    Ok(())
}

fn format_workspace(path: &Path, check: bool) -> Result<u8> {
    let workspace = Workspace::discover(path)?;
    let requests = workspace.requests()?;
    let mut by_file: BTreeMap<PathBuf, Vec<HttpRequest>> = BTreeMap::new();
    for request in requests {
        by_file
            .entry(request.source.clone())
            .or_default()
            .push(request);
    }
    let mut changed = Vec::new();
    for (path, requests) in by_file {
        let existing = fs::read_to_string(&path)?;
        let formatted = format_document(&requests);
        if existing != formatted {
            changed.push(path.clone());
            if !check {
                fs::write(path, formatted)?;
            }
        }
    }
    if check && !changed.is_empty() {
        for path in changed {
            eprintln!("would reformat {}", path.display());
        }
        return Ok(5);
    }
    println!("{} request files formatted", changed.len());
    Ok(0)
}

fn import(command: ImportCommand) -> Result<u8> {
    match command {
        ImportCommand::Curl { command, output } => {
            let request = import_curl_command(&command)?;
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&output, format_document(&[request]))?;
            println!("Imported {}", output.display());
            Ok(0)
        }
        ImportCommand::Postman { input, output } => import_postman(&input, &output),
    }
}

fn import_postman(input: &Path, output: &Path) -> Result<u8> {
    let document: serde_json::Value = serde_json::from_slice(&fs::read(input)?)
        .with_context(|| format!("parsing {}", input.display()))?;
    let schema = document
        .pointer("/info/schema")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if !schema.contains("v2.1") {
        bail!("only Postman Collection v2.1 is currently supported");
    }
    fs::create_dir_all(output)?;
    let mut imported = Vec::new();
    let mut losses = Vec::new();
    collect_postman_items(
        document
            .get("item")
            .and_then(serde_json::Value::as_array)
            .unwrap_or(&Vec::new()),
        &mut Vec::new(),
        &mut imported,
        &mut losses,
    )?;
    for (index, (path, request)) in imported.iter().enumerate() {
        let mut target = output.to_path_buf();
        for part in path {
            target.push(safe_filename(part));
        }
        fs::create_dir_all(&target)?;
        target.push(format!(
            "{:03}-{}.http",
            index + 1,
            safe_filename(&request.name)
        ));
        if target.exists() {
            bail!("refusing to overwrite {}", target.display());
        }
        fs::write(&target, format_document(std::slice::from_ref(request)))?;
    }
    println!(
        "Imported {} requests into {}",
        imported.len(),
        output.display()
    );
    if !losses.is_empty() {
        eprintln!(
            "Import completed with {} compatibility notes:",
            losses.len()
        );
        for loss in losses {
            eprintln!("- {loss}");
        }
        return Ok(5);
    }
    Ok(0)
}

fn collect_postman_items(
    items: &[serde_json::Value],
    path: &mut Vec<String>,
    imported: &mut Vec<(Vec<String>, HttpRequest)>,
    losses: &mut Vec<String>,
) -> Result<()> {
    for item in items {
        let name = item
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("request");
        if let Some(children) = item.get("item").and_then(serde_json::Value::as_array) {
            path.push(name.into());
            collect_postman_items(children, path, imported, losses)?;
            path.pop();
            continue;
        }
        let Some(request) = item.get("request") else {
            losses.push(format!("{name}: item has no request"));
            continue;
        };
        let method = request
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("GET");
        let url = request
            .get("url")
            .and_then(|url| {
                url.as_str()
                    .or_else(|| url.get("raw").and_then(serde_json::Value::as_str))
            })
            .ok_or_else(|| anyhow!("Postman request `{name}` has no raw URL"))?;
        let headers: Vec<Header> = request
            .get("header")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter(|header| {
                !header
                    .get("disabled")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            })
            .filter_map(|header| {
                Some(Header {
                    name: header.get("key")?.as_str()?.into(),
                    value: header.get("value")?.as_str()?.into(),
                })
            })
            .collect();
        let body = request
            .pointer("/body/raw")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if request.get("auth").is_some_and(|auth| !auth.is_null()) {
            losses.push(format!("{name}: auth metadata requires manual review"));
        }
        if item
            .get("event")
            .is_some_and(|events| events.as_array().is_some_and(|events| !events.is_empty()))
        {
            losses.push(format!(
                "{name}: scripts require the forthcoming sandbox compatibility layer"
            ));
        }
        imported.push((
            path.clone(),
            one_off_request(
                method.into(),
                url.into(),
                headers
                    .into_iter()
                    .map(|header| format!("{}: {}", header.name, header.value))
                    .collect(),
                (!body.is_empty()).then_some(body),
            )?,
        ));
        if let Some((_, imported_request)) = imported.last_mut() {
            imported_request.name = name.into();
        }
    }
    Ok(())
}

fn safe_filename(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if value.is_empty() {
        "request".into()
    } else {
        value
    }
}

fn vault(command: VaultCommand, path: &Path) -> Result<u8> {
    let workspace = Workspace::discover(path)?;
    let index_path = workspace.root.join(".verbsmith/vault.toml");
    let mut index = if index_path.exists() {
        toml::from_str::<VaultIndex>(&fs::read_to_string(&index_path)?)?
    } else {
        VaultIndex::default()
    };
    match command {
        VaultCommand::Set { name } => {
            validate_secret_name(&name)?;
            let value = rpassword::prompt_password(format!("Value for {name}: "))?;
            if value.is_empty() {
                bail!("secret value cannot be empty");
            }
            vault_entry(&workspace, &name)?.set_password(&value)?;
            if !index.names.contains(&name) {
                index.names.push(name.clone());
                index.names.sort();
            }
            persist_vault_index(&index_path, &index)?;
            println!("Stored {name}");
        }
        VaultCommand::Get { name } => {
            validate_secret_name(&name)?;
            vault_entry(&workspace, &name)?.get_password()?;
            println!("{name} is available");
        }
        VaultCommand::Remove { name } => {
            validate_secret_name(&name)?;
            vault_entry(&workspace, &name)?.delete_credential()?;
            index.names.retain(|candidate| candidate != &name);
            persist_vault_index(&index_path, &index)?;
            println!("Removed {name}");
        }
        VaultCommand::List => {
            for name in index.names {
                println!("{name}");
            }
        }
    }
    Ok(0)
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct VaultIndex {
    #[serde(default)]
    names: Vec<String>,
}

pub(crate) fn vault_entry(workspace: &Workspace, name: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(
        "dev.verbsmith.vault",
        &format!("{}:{name}", workspace.manifest.name),
    )
    .map_err(Into::into)
}

fn persist_vault_index(path: &Path, index: &VaultIndex) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml::to_string_pretty(index)?)?;
    Ok(())
}

fn validate_secret_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
    {
        bail!("secret names may contain only letters, digits, dots, dashes, and underscores");
    }
    Ok(())
}

pub(crate) fn resolve_secret_references(
    workspace: &Workspace,
    variables: &mut BTreeMap<String, String>,
) -> Result<Vec<String>> {
    let mut redactions = Vec::new();
    for value in variables.values_mut() {
        let Some(reference) = value.strip_prefix("secret://") else {
            continue;
        };
        let name = reference
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| anyhow!("invalid secret reference"))?;
        validate_secret_name(name)?;
        *value = vault_entry(workspace, name)?
            .get_password()
            .with_context(|| {
                format!("reading secret `{name}` from the operating-system credential manager")
            })?;
        redactions.push(value.clone());
    }
    Ok(redactions)
}

fn import_curl_command(command: &str) -> Result<HttpRequest> {
    let tokens = shell_words(command)?;
    let mut iterator = tokens.into_iter();
    if iterator.next().as_deref() != Some("curl") {
        bail!("command must begin with `curl`");
    }
    let mut method = None;
    let mut url = None;
    let mut headers = Vec::new();
    let mut body = None;
    while let Some(token) = iterator.next() {
        match token.as_str() {
            "-X" | "--request" => method = iterator.next(),
            "-H" | "--header" => {
                let value = iterator
                    .next()
                    .ok_or_else(|| anyhow!("missing header value"))?;
                let (name, value) = value
                    .split_once(':')
                    .ok_or_else(|| anyhow!("invalid curl header"))?;
                headers.push(Header {
                    name: name.trim().into(),
                    value: value.trim().into(),
                });
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" => body = iterator.next(),
            value if value.starts_with('-') => {
                bail!("unsupported curl option `{value}`; import stopped without losing it")
            }
            value => url = Some(value.to_owned()),
        }
    }
    let method = method.unwrap_or_else(|| {
        if body.is_some() {
            "POST".into()
        } else {
            "GET".into()
        }
    });
    one_off_request(
        method,
        url.ok_or_else(|| anyhow!("curl command has no URL"))?,
        headers
            .into_iter()
            .map(|h| format!("{}: {}", h.name, h.value))
            .collect(),
        body,
    )
}

fn shell_words(input: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in input.chars() {
        if escaped {
            word.push(character);
            escaped = false;
        } else if character == '\\' && quote != Some('\'') {
            escaped = true;
        } else if quote == Some(character) {
            quote = None;
        } else if quote.is_none() && (character == '\'' || character == '"') {
            quote = Some(character);
        } else if quote.is_none() && character.is_whitespace() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(character);
        }
    }
    if escaped || quote.is_some() {
        bail!("unterminated quote or escape in curl command");
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

fn doctor() -> Result<u8> {
    println!("Verbsmith {}", env!("CARGO_PKG_VERSION"));
    println!("platform: {}-{}", env::consts::ARCH, env::consts::OS);
    println!("libcurl: {}", curl::Version::get().version());
    println!(
        "ssl: {}",
        curl::Version::get().ssl_version().unwrap_or("unknown")
    );
    println!("http2: {}", curl::Version::get().feature_http2());
    println!("http3: {}", curl::Version::get().feature_http3());
    println!("telemetry: disabled");
    Ok(0)
}

fn parse_pair(value: &str) -> Result<(String, String), String> {
    let (key, value) = value
        .split_once('=')
        .ok_or_else(|| "expected NAME=VALUE".to_owned())?;
    if key.is_empty() {
        return Err("variable name cannot be empty".into());
    }
    Ok((key.into(), value.into()))
}

fn classify_error(error: &anyhow::Error) -> u8 {
    if let Some(error) = error.downcast_ref::<verbsmith_core::Error>() {
        match error {
            verbsmith_core::Error::Transport(_) | verbsmith_core::Error::InvalidUrl(_) => 3,
            verbsmith_core::Error::Parse { .. }
            | verbsmith_core::Error::UnsupportedSchema { .. } => 5,
            _ => 6,
        }
    } else {
        6
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_common_curl_command() {
        let request = import_curl_command("curl -X POST -H 'Content-Type: application/json' -d '{\"ok\":true}' https://example.com").unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.headers[0].name, "Content-Type");
        assert_eq!(request.body, r#"{"ok":true}"#);
    }

    #[test]
    fn imports_postman_collection_without_silent_loss() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("collection.json");
        fs::write(
            &input,
            r#"{
              "info":{"name":"Example","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
              "item":[{"name":"Health","request":{"method":"GET","header":[],"url":{"raw":"https://example.com/health"}}}]
            }"#,
        )
        .unwrap();
        let output = directory.path().join("requests");
        assert_eq!(import_postman(&input, &output).unwrap(), 0);
        let imported = fs::read_to_string(output.join("001-health.http")).unwrap();
        assert!(imported.contains("# @name Health"));
        assert!(imported.contains("GET https://example.com/health"));
    }
}
