use std::env;
use std::fmt;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

type AppResult<T> = Result<T, AppError>;

#[derive(Debug)]
enum AppError {
    Io(io::Error),
    Message(String),
    CommandFailed {
        program: String,
        args: Vec<String>,
        status: ExitStatus,
        stderr: String,
    },
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Message(message) => write!(f, "{message}"),
            Self::CommandFailed {
                program,
                args,
                status,
                stderr,
            } => {
                write!(
                    f,
                    "command failed: {} {} (status: {})",
                    program,
                    args.iter()
                        .map(|arg| shell_quote(arg))
                        .collect::<Vec<_>>()
                        .join(" "),
                    status
                )?;

                if !stderr.trim().is_empty() {
                    write!(f, "\nstderr:\n{}", stderr.trim())?;
                }

                Ok(())
            }
        }
    }
}

impl std::error::Error for AppError {}

impl From<io::Error> for AppError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone)]
struct GlobalOptions {
    pdnsutil_bin: String,
    config_dir: Option<String>,
    config_name: Option<String>,
    dry_run: bool,
}

impl Default for GlobalOptions {
    fn default() -> Self {
        Self {
            pdnsutil_bin: "pdnsutil".to_string(),
            config_dir: None,
            config_name: None,
            dry_run: false,
        }
    }
}

#[derive(Debug)]
struct Cli {
    global: GlobalOptions,
    command: Option<CommandKind>,
}

#[derive(Debug)]
enum CommandKind {
    AddRecord(AddRecordArgs),
    DeleteRecord(DeleteRecordArgs),
    ListZones,
    ListRecords(ListRecordsArgs),
    Help,
}

#[derive(Debug, Default)]
struct AddRecordArgs {
    zone: Option<String>,
    name: Option<String>,
    record_type: Option<String>,
    content: Option<String>,
    ttl: Option<u32>,
    yes: bool,
}

#[derive(Debug, Default)]
struct DeleteRecordArgs {
    zone: Option<String>,
    name: Option<String>,
    record_type: Option<String>,
    content: Option<String>,
    yes: bool,
}

#[derive(Debug, Default)]
struct ListRecordsArgs {
    zone: Option<String>,
}

#[derive(Debug, Clone)]
struct AddRecordSpec {
    zone: String,
    name: String,
    record_type: String,
    content: String,
    ttl: Option<u32>,
}

#[derive(Debug, Clone)]
struct DeleteRecordSpec {
    zone: String,
    name: String,
    record_type: String,
    content: String,
}

#[derive(Debug, Clone)]
struct ZoneRecord {
    name: String,
    ttl: Option<u32>,
    record_type: String,
    content: String,
}

#[derive(Debug, Clone)]
enum DeleteMethod {
    DeleteRrset,
    Replace {
        ttl: Option<u32>,
        remaining_contents: Vec<String>,
    },
}

#[derive(Debug, Clone)]
struct DeletePlan {
    zone: String,
    name: String,
    record_type: String,
    method: DeleteMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PdnsSyntax {
    Modern,
    Legacy,
}

struct PdnsUtil {
    global: GlobalOptions,
    syntax: PdnsSyntax,
}

const PPDNS_RELEASE_REPO: &str = "0x3st/ppdns";

#[derive(Debug, Clone, Copy)]
enum HomeAction {
    AddRecord,
    DeleteRecord,
    ListZones,
    ListRecords,
    InstallPowerDns,
    UpdatePowerDns,
    ReinstallPowerDns,
    UpdatePpdns,
    ReinstallPpdns,
    Exit,
}

#[derive(Debug, Clone, Copy)]
enum PackageAction {
    Install,
    Update,
    Reinstall,
}

#[derive(Debug, Clone)]
enum PowerDnsStatus {
    NotInstalled {
        candidate: Option<String>,
    },
    Installed {
        installed: String,
        candidate: Option<String>,
    },
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone)]
enum SelfStatus {
    LatestKnown {
        current: String,
        latest: String,
        update_available: bool,
    },
    UnknownLatest {
        current: String,
        reason: String,
    },
}

#[derive(Debug, Clone)]
struct HomeStatus {
    powerdns: PowerDnsStatus,
    ppdns: SelfStatus,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

fn run() -> AppResult<()> {
    let cli = Cli::parse(env::args().skip(1).collect())?;
    cli.execute()
}

impl Cli {
    fn parse(args: Vec<String>) -> AppResult<Self> {
        let mut cursor = ArgCursor::new(args);
        let global = parse_global_options(&mut cursor)?;

        let command = match cursor.next() {
            None => None,
            Some(command) if is_help(&command) => Some(CommandKind::Help),
            Some(command) if matches!(command.as_str(), "add" | "create") => {
                let object = cursor
                    .next()
                    .ok_or_else(|| AppError::Message("missing object after `add`".to_string()))?;

                match object.as_str() {
                    "record" | "rrset" => {
                        Some(CommandKind::AddRecord(parse_add_record_args(&mut cursor)?))
                    }
                    _ => {
                        return Err(AppError::Message(format!(
                            "unsupported add target: `{object}`"
                        )))
                    }
                }
            }
            Some(command) if matches!(command.as_str(), "delete" | "del" | "remove" | "rm") => {
                let object = cursor.next().ok_or_else(|| {
                    AppError::Message("missing object after `delete`".to_string())
                })?;

                match object.as_str() {
                    "record" | "rrset" => Some(CommandKind::DeleteRecord(
                        parse_delete_record_args(&mut cursor)?,
                    )),
                    _ => {
                        return Err(AppError::Message(format!(
                            "unsupported delete target: `{object}`"
                        )))
                    }
                }
            }
            Some(command) if matches!(command.as_str(), "list" | "ls") => {
                let object = cursor
                    .next()
                    .ok_or_else(|| AppError::Message("missing object after `list`".to_string()))?;

                match object.as_str() {
                    "zones" => Some(CommandKind::ListZones),
                    "records" => Some(CommandKind::ListRecords(parse_list_records_args(
                        &mut cursor,
                    )?)),
                    _ => {
                        return Err(AppError::Message(format!(
                            "unsupported list target: `{object}`"
                        )))
                    }
                }
            }
            Some(command) => {
                return Err(AppError::Message(format!("unknown command: `{command}`")))
            }
        };

        if let Some(unused) = cursor.next() {
            return Err(AppError::Message(format!(
                "unexpected argument: `{unused}`"
            )));
        }

        Ok(Self { global, command })
    }

    fn execute(self) -> AppResult<()> {
        match self.command {
            Some(CommandKind::Help) => {
                print_help();
                Ok(())
            }
            None => {
                if !stdin_is_interactive() {
                    print_help();
                    return Ok(());
                }
                interactive_home(&self.global)
            }
            Some(CommandKind::AddRecord(args)) => {
                let runner = PdnsUtil::new(self.global)?;
                execute_add_record(&runner, args)
            }
            Some(CommandKind::DeleteRecord(args)) => {
                let runner = PdnsUtil::new(self.global)?;
                execute_delete_record(&runner, args)
            }
            Some(CommandKind::ListZones) => {
                let runner = PdnsUtil::new(self.global)?;
                print_zones(&runner.list_zones()?)
            }
            Some(CommandKind::ListRecords(args)) => {
                let runner = PdnsUtil::new(self.global)?;
                execute_list_records(&runner, args)
            }
        }
    }
}

impl PdnsUtil {
    fn new(global: GlobalOptions) -> AppResult<Self> {
        match Command::new(&global.pdnsutil_bin).arg("--help").output() {
            Ok(output) => {
                let mut help_output = String::from_utf8_lossy(&output.stdout).into_owned();
                if !output.stderr.is_empty() {
                    if !help_output.is_empty() {
                        help_output.push('\n');
                    }
                    help_output.push_str(&String::from_utf8_lossy(&output.stderr));
                }

                Ok(Self {
                    global,
                    syntax: detect_pdns_syntax(&help_output),
                })
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Err(AppError::Message(format!(
                "cannot find `{}`. Install pdnsutil or pass `--pdnsutil /path/to/pdnsutil`.",
                global.pdnsutil_bin
            ))),
            Err(err) => Err(AppError::Io(err)),
        }
    }

    fn list_zones(&self) -> AppResult<Vec<String>> {
        let args = self.list_zones_args();
        let output = self.run_capture(&args)?;
        let mut zones: Vec<String> = output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(normalize_zone_name)
            .collect();
        zones.sort();
        zones.dedup();
        Ok(zones)
    }

    fn list_zone_records(&self, zone: &str) -> AppResult<Vec<ZoneRecord>> {
        let args = self.list_zone_records_args(zone);
        let output = self.run_capture(&args)?;
        let mut current_origin = normalize_zone_name(zone);
        let mut records = Vec::new();

        for line in output.lines() {
            let trimmed = strip_comment_preserving_quotes(line);
            if trimmed.is_empty() {
                continue;
            }

            if let Some(origin) = parse_origin_directive(&trimmed, &current_origin) {
                current_origin = origin;
                continue;
            }

            if trimmed.starts_with("$TTL") || trimmed.contains('(') || trimmed.contains(')') {
                continue;
            }

            if let Some(record) = parse_zone_record_line(&trimmed, &current_origin) {
                records.push(record);
            }
        }

        Ok(records)
    }

    fn add_record(&self, spec: &AddRecordSpec) -> AppResult<()> {
        let args = self.add_record_args(spec);
        self.run_status(&args)
    }

    fn apply_delete_plan(&self, plan: &DeletePlan) -> AppResult<()> {
        let args = self.delete_plan_args(plan);
        self.run_status(&args)
    }

    fn preview_command(&self, args: &[String]) -> String {
        let mut full = vec![self.global.pdnsutil_bin.clone()];

        if let Some(config_dir) = &self.global.config_dir {
            full.push("--config-dir".to_string());
            full.push(config_dir.clone());
        }

        if let Some(config_name) = &self.global.config_name {
            full.push("--config-name".to_string());
            full.push(config_name.clone());
        }

        full.extend(args.iter().cloned());
        full.into_iter()
            .map(|item| shell_quote(&item))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn list_zones_args(&self) -> Vec<String> {
        match self.syntax {
            PdnsSyntax::Modern => vec!["zone".to_string(), "list-all".to_string()],
            PdnsSyntax::Legacy => vec!["list-all-zones".to_string()],
        }
    }

    fn list_zone_records_args(&self, zone: &str) -> Vec<String> {
        match self.syntax {
            PdnsSyntax::Modern => {
                vec!["zone".to_string(), "list".to_string(), zone.to_string()]
            }
            PdnsSyntax::Legacy => vec!["list-zone".to_string(), zone.to_string()],
        }
    }

    fn add_record_args(&self, spec: &AddRecordSpec) -> Vec<String> {
        let mut args = match self.syntax {
            PdnsSyntax::Modern => vec![
                "rrset".to_string(),
                "add".to_string(),
                spec.zone.clone(),
                spec.name.clone(),
                spec.record_type.clone(),
            ],
            PdnsSyntax::Legacy => vec![
                "add-record".to_string(),
                spec.zone.clone(),
                spec.name.clone(),
                spec.record_type.clone(),
            ],
        };

        if let Some(ttl) = spec.ttl {
            args.push(ttl.to_string());
        }

        args.push(spec.content.clone());
        args
    }

    fn delete_plan_args(&self, plan: &DeletePlan) -> Vec<String> {
        match (&self.syntax, &plan.method) {
            (PdnsSyntax::Modern, DeleteMethod::DeleteRrset) => vec![
                "rrset".to_string(),
                "delete".to_string(),
                plan.zone.clone(),
                plan.name.clone(),
                plan.record_type.clone(),
            ],
            (PdnsSyntax::Legacy, DeleteMethod::DeleteRrset) => vec![
                "delete-rrset".to_string(),
                plan.zone.clone(),
                plan.name.clone(),
                plan.record_type.clone(),
            ],
            (
                PdnsSyntax::Modern,
                DeleteMethod::Replace {
                    ttl,
                    remaining_contents,
                },
            ) => {
                let mut args = vec![
                    "rrset".to_string(),
                    "replace".to_string(),
                    plan.zone.clone(),
                    plan.name.clone(),
                    plan.record_type.clone(),
                ];

                if let Some(ttl) = ttl {
                    args.push(ttl.to_string());
                }

                args.extend(remaining_contents.iter().cloned());
                args
            }
            (
                PdnsSyntax::Legacy,
                DeleteMethod::Replace {
                    ttl,
                    remaining_contents,
                },
            ) => {
                let mut args = vec![
                    "replace-rrset".to_string(),
                    plan.zone.clone(),
                    plan.name.clone(),
                    plan.record_type.clone(),
                ];

                if let Some(ttl) = ttl {
                    args.push(ttl.to_string());
                }

                args.extend(remaining_contents.iter().cloned());
                args
            }
        }
    }

    fn run_capture(&self, args: &[String]) -> AppResult<String> {
        let mut command = Command::new(&self.global.pdnsutil_bin);
        self.apply_global_args(&mut command);
        command.args(args);
        let output = command.output()?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(AppError::CommandFailed {
                program: self.global.pdnsutil_bin.clone(),
                args: args.to_vec(),
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }

    fn run_status(&self, args: &[String]) -> AppResult<()> {
        if self.global.dry_run {
            println!("DRY RUN");
            println!("{}", self.preview_command(args));
            return Ok(());
        }

        let mut command = Command::new(&self.global.pdnsutil_bin);
        self.apply_global_args(&mut command);
        command.args(args);
        let output = command.output()?;

        if output.status.success() {
            if !output.stdout.is_empty() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stdout = stdout.trim();
                if !stdout.is_empty() {
                    println!("{stdout}");
                }
            }

            Ok(())
        } else {
            Err(AppError::CommandFailed {
                program: self.global.pdnsutil_bin.clone(),
                args: args.to_vec(),
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }

    fn apply_global_args(&self, command: &mut Command) {
        if let Some(config_dir) = &self.global.config_dir {
            command.arg("--config-dir").arg(config_dir);
        }

        if let Some(config_name) = &self.global.config_name {
            command.arg("--config-name").arg(config_name);
        }
    }
}

struct ArgCursor {
    args: Vec<String>,
    index: usize,
}

impl ArgCursor {
    fn new(args: Vec<String>) -> Self {
        Self { args, index: 0 }
    }

    fn next(&mut self) -> Option<String> {
        let value = self.args.get(self.index).cloned();
        self.index += usize::from(value.is_some());
        value
    }

    fn peek(&self) -> Option<&str> {
        self.args.get(self.index).map(String::as_str)
    }
}

fn parse_global_options(cursor: &mut ArgCursor) -> AppResult<GlobalOptions> {
    let mut global = GlobalOptions::default();

    loop {
        match cursor.peek() {
            Some("--pdnsutil") => {
                cursor.next();
                global.pdnsutil_bin = expect_value(cursor, "--pdnsutil")?;
            }
            Some("--config-dir") => {
                cursor.next();
                global.config_dir = Some(expect_value(cursor, "--config-dir")?);
            }
            Some("--config-name") => {
                cursor.next();
                global.config_name = Some(expect_value(cursor, "--config-name")?);
            }
            Some("--dry-run") => {
                cursor.next();
                global.dry_run = true;
            }
            Some("-h" | "--help") => break,
            Some(value) if value.starts_with('-') => {
                return Err(AppError::Message(format!(
                    "unknown global option: `{value}`"
                )))
            }
            _ => break,
        }
    }

    Ok(global)
}

fn parse_add_record_args(cursor: &mut ArgCursor) -> AppResult<AddRecordArgs> {
    let mut args = AddRecordArgs::default();

    while let Some(token) = cursor.peek() {
        match token {
            "--zone" | "-z" => {
                cursor.next();
                args.zone = Some(expect_value(cursor, "--zone")?);
            }
            "--name" | "-n" => {
                cursor.next();
                args.name = Some(expect_value(cursor, "--name")?);
            }
            "--type" | "-t" => {
                cursor.next();
                args.record_type = Some(expect_value(cursor, "--type")?);
            }
            "--content" | "-c" => {
                cursor.next();
                args.content = Some(expect_value(cursor, "--content")?);
            }
            "--ttl" => {
                cursor.next();
                let ttl = expect_value(cursor, "--ttl")?;
                args.ttl = Some(parse_ttl(&ttl)?);
            }
            "-y" | "--yes" => {
                cursor.next();
                args.yes = true;
            }
            "-h" | "--help" => {
                cursor.next();
                return Err(AppError::Message(
                    "run `ppdns --help` for usage".to_string(),
                ));
            }
            _ => break,
        }
    }

    Ok(args)
}

fn parse_delete_record_args(cursor: &mut ArgCursor) -> AppResult<DeleteRecordArgs> {
    let mut args = DeleteRecordArgs::default();

    while let Some(token) = cursor.peek() {
        match token {
            "--zone" | "-z" => {
                cursor.next();
                args.zone = Some(expect_value(cursor, "--zone")?);
            }
            "--name" | "-n" => {
                cursor.next();
                args.name = Some(expect_value(cursor, "--name")?);
            }
            "--type" | "-t" => {
                cursor.next();
                args.record_type = Some(expect_value(cursor, "--type")?);
            }
            "--content" | "-c" => {
                cursor.next();
                args.content = Some(expect_value(cursor, "--content")?);
            }
            "-y" | "--yes" => {
                cursor.next();
                args.yes = true;
            }
            "-h" | "--help" => {
                cursor.next();
                return Err(AppError::Message(
                    "run `ppdns --help` for usage".to_string(),
                ));
            }
            _ => break,
        }
    }

    Ok(args)
}

fn parse_list_records_args(cursor: &mut ArgCursor) -> AppResult<ListRecordsArgs> {
    let mut args = ListRecordsArgs::default();

    while let Some(token) = cursor.peek() {
        match token {
            "--zone" | "-z" => {
                cursor.next();
                args.zone = Some(expect_value(cursor, "--zone")?);
            }
            _ => break,
        }
    }

    Ok(args)
}

fn execute_add_record(runner: &PdnsUtil, args: AddRecordArgs) -> AppResult<()> {
    let skip_confirmation = args.yes;
    let spec = resolve_add_record_spec(runner, args)?;

    let preview = runner.add_record_args(&spec);
    println!("Ready to add:");
    println!("  zone:    {}", spec.zone);
    println!("  name:    {}", spec.name);
    println!("  type:    {}", spec.record_type);
    println!("  content: {}", spec.content);
    println!(
        "  ttl:     {}",
        spec.ttl
            .map(|ttl| ttl.to_string())
            .unwrap_or_else(|| "<default>".to_string())
    );
    println!("  command: {}", runner.preview_command(&preview));

    if !skip_confirmation && !prompt_confirm("Apply this change?", true)? {
        println!("Cancelled.");
        return Ok(());
    }

    runner.add_record(&spec)?;
    println!("Record added.");
    Ok(())
}

fn execute_delete_record(runner: &PdnsUtil, args: DeleteRecordArgs) -> AppResult<()> {
    let skip_confirmation = args.yes;
    let (spec, plan) = resolve_delete_record_plan(runner, args)?;

    let preview = runner.delete_plan_args(&plan);
    println!("Ready to delete:");
    println!("  zone:    {}", spec.zone);
    println!("  name:    {}", spec.name);
    println!("  type:    {}", spec.record_type);
    println!("  content: {}", spec.content);

    match &plan.method {
        DeleteMethod::DeleteRrset => {
            println!("  method:  delete full rrset (selected value is the last one)");
        }
        DeleteMethod::Replace {
            ttl,
            remaining_contents,
        } => {
            println!(
                "  method:  replace rrset and keep {} remaining value(s)",
                remaining_contents.len()
            );
            println!(
                "  ttl:     {}",
                ttl.map(|value| value.to_string())
                    .unwrap_or_else(|| "<default>".to_string())
            );
        }
    }

    println!("  command: {}", runner.preview_command(&preview));

    if !skip_confirmation && !prompt_confirm("Apply this change?", true)? {
        println!("Cancelled.");
        return Ok(());
    }

    runner.apply_delete_plan(&plan)?;
    println!("Record deleted.");
    Ok(())
}

fn execute_list_records(runner: &PdnsUtil, args: ListRecordsArgs) -> AppResult<()> {
    let zone = match args.zone {
        Some(zone) => normalize_zone_name(&zone),
        None => {
            if !stdin_is_interactive() {
                return Err(AppError::Message(
                    "missing `--zone` and stdin is not interactive".to_string(),
                ));
            }
            select_zone(runner)?
        }
    };

    let records = runner.list_zone_records(&zone)?;
    print_records(&zone, &records)
}

fn interactive_home(global: &GlobalOptions) -> AppResult<()> {
    loop {
        let status = gather_home_status();
        print_home_status(&status);

        let actions = build_home_actions(&status);
        let labels: Vec<String> = actions.iter().map(|(label, _)| label.clone()).collect();
        let choice = prompt_select("Choose an action", &labels)?;

        match actions[choice].1 {
            HomeAction::AddRecord => {
                let runner = PdnsUtil::new(global.clone())?;
                execute_add_record(&runner, AddRecordArgs::default())?;
            }
            HomeAction::DeleteRecord => {
                let runner = PdnsUtil::new(global.clone())?;
                execute_delete_record(&runner, DeleteRecordArgs::default())?;
            }
            HomeAction::ListZones => {
                let runner = PdnsUtil::new(global.clone())?;
                print_zones(&runner.list_zones()?)?;
            }
            HomeAction::ListRecords => {
                let runner = PdnsUtil::new(global.clone())?;
                execute_list_records(&runner, ListRecordsArgs::default())?;
            }
            HomeAction::InstallPowerDns => {
                execute_powerdns_package_action(global, PackageAction::Install)?;
            }
            HomeAction::UpdatePowerDns => {
                execute_powerdns_package_action(global, PackageAction::Update)?;
            }
            HomeAction::ReinstallPowerDns => {
                execute_powerdns_package_action(global, PackageAction::Reinstall)?;
            }
            HomeAction::UpdatePpdns => {
                execute_self_update_action(global, false, Some(&status.ppdns))?;
            }
            HomeAction::ReinstallPpdns => {
                execute_self_update_action(global, true, Some(&status.ppdns))?;
            }
            HomeAction::Exit => return Ok(()),
        }
    }
}

fn gather_home_status() -> HomeStatus {
    HomeStatus {
        powerdns: detect_powerdns_status(),
        ppdns: detect_self_status(),
    }
}

fn print_home_status(status: &HomeStatus) {
    println!("Status:");

    match &status.powerdns {
        PowerDnsStatus::NotInstalled { candidate } => {
            if let Some(candidate) = candidate {
                println!("  PowerDNS: not installed, candidate in current repos: {candidate}");
            } else {
                println!("  PowerDNS: not installed");
            }
        }
        PowerDnsStatus::Installed {
            installed,
            candidate,
        } => {
            if let Some(candidate) = candidate {
                if candidate != installed {
                    println!(
                        "  PowerDNS: installed {installed}, candidate {candidate} in current repos"
                    );
                } else {
                    println!("  PowerDNS: installed {installed}, up to date in current repos");
                }
            } else {
                println!("  PowerDNS: installed {installed}");
            }
        }
        PowerDnsStatus::Unsupported { reason } => {
            println!("  PowerDNS: status check unavailable ({reason})");
        }
    }

    match &status.ppdns {
        SelfStatus::LatestKnown {
            current,
            latest,
            update_available,
        } => {
            if *update_available {
                println!("  ppdns: current {current}, latest {latest}");
            } else {
                println!("  ppdns: current {current}, up to date");
            }
        }
        SelfStatus::UnknownLatest { current, reason } => {
            println!("  ppdns: current {current}, latest check unavailable ({reason})");
        }
    }

    println!();
}

fn build_home_actions(status: &HomeStatus) -> Vec<(String, HomeAction)> {
    let mut actions = vec![
        ("Add record".to_string(), HomeAction::AddRecord),
        ("Delete record".to_string(), HomeAction::DeleteRecord),
        ("List zones".to_string(), HomeAction::ListZones),
        ("List records".to_string(), HomeAction::ListRecords),
    ];

    match &status.powerdns {
        PowerDnsStatus::NotInstalled { .. } => {
            actions.push(("Install PowerDNS".to_string(), HomeAction::InstallPowerDns));
        }
        PowerDnsStatus::Installed {
            installed,
            candidate,
        } => {
            if candidate
                .as_ref()
                .is_some_and(|candidate| candidate != installed)
            {
                actions.push(("Update PowerDNS".to_string(), HomeAction::UpdatePowerDns));
            }
            actions.push((
                "Reinstall PowerDNS".to_string(),
                HomeAction::ReinstallPowerDns,
            ));
        }
        PowerDnsStatus::Unsupported { .. } => {}
    }

    match &status.ppdns {
        SelfStatus::LatestKnown {
            update_available: true,
            ..
        } => {
            actions.push(("Update ppdns".to_string(), HomeAction::UpdatePpdns));
            actions.push(("Reinstall ppdns".to_string(), HomeAction::ReinstallPpdns));
        }
        SelfStatus::LatestKnown { .. } | SelfStatus::UnknownLatest { .. } => {
            actions.push(("Reinstall ppdns".to_string(), HomeAction::ReinstallPpdns));
        }
    }

    actions.push(("Exit".to_string(), HomeAction::Exit));
    actions
}

fn resolve_add_record_spec(runner: &PdnsUtil, args: AddRecordArgs) -> AppResult<AddRecordSpec> {
    let interactive = stdin_is_interactive();
    let zone = resolve_zone(runner, args.zone, interactive)?;
    let record_type = match args.record_type {
        Some(record_type) => normalize_record_type(&record_type),
        None if interactive => prompt_record_type()?,
        None => {
            return Err(AppError::Message(
                "missing `--type` and stdin is not interactive".to_string(),
            ))
        }
    };

    let name = match args.name {
        Some(name) => normalize_owner_name(&name, &zone),
        None if interactive => {
            let raw = prompt_input("Record name (@ for zone apex)", Some("@"))?;
            normalize_owner_name(&raw, &zone)
        }
        None => {
            return Err(AppError::Message(
                "missing `--name` and stdin is not interactive".to_string(),
            ))
        }
    };

    let ttl = match args.ttl {
        Some(ttl) => Some(ttl),
        None if interactive => prompt_optional_ttl()?,
        None => None,
    };

    let content = match args.content {
        Some(content) => content,
        None if interactive => prompt_content_for_type(&record_type, &zone)?,
        None => {
            return Err(AppError::Message(
                "missing `--content` and stdin is not interactive".to_string(),
            ))
        }
    };

    Ok(AddRecordSpec {
        zone,
        name,
        record_type,
        content,
        ttl,
    })
}

fn resolve_delete_record_plan(
    runner: &PdnsUtil,
    args: DeleteRecordArgs,
) -> AppResult<(DeleteRecordSpec, DeletePlan)> {
    let interactive = stdin_is_interactive();
    let zone = resolve_zone(runner, args.zone, interactive)?;
    let records = runner.list_zone_records(&zone)?;

    if records.is_empty() {
        return Err(AppError::Message(format!(
            "zone `{zone}` has no parseable records"
        )));
    }

    let name = args
        .name
        .as_ref()
        .map(|name| normalize_owner_name(name, &zone));
    let record_type = args
        .record_type
        .as_ref()
        .map(|record_type| normalize_record_type(record_type));
    let content = args.content.clone();

    let initial_matches = filter_records(
        &records,
        name.as_deref(),
        record_type.as_deref(),
        content.as_deref(),
    );

    let selected = if initial_matches.len() == 1 {
        initial_matches[0].clone()
    } else if let (Some(name), Some(record_type), Some(content)) =
        (name.as_deref(), record_type.as_deref(), content.as_deref())
    {
        find_record_exact(&records, name, record_type, content)?
    } else if interactive {
        select_record_for_delete(
            &records,
            name.as_deref(),
            record_type.as_deref(),
            content.as_deref(),
        )?
    } else if initial_matches.is_empty() {
        return Err(AppError::Message(
            "no matching record found; pass exact `--name`, `--type`, `--content` or run in an interactive terminal".to_string(),
        ));
    } else {
        return Err(AppError::Message(format!(
            "record selection is ambiguous ({} matches); pass `--content` or run in an interactive terminal",
            initial_matches.len()
        )));
    };

    let spec = DeleteRecordSpec {
        zone: zone.clone(),
        name: selected.name.clone(),
        record_type: selected.record_type.clone(),
        content: selected.content.clone(),
    };
    let plan = build_delete_plan(&zone, &records, &spec)?;
    Ok((spec, plan))
}

fn resolve_zone(runner: &PdnsUtil, zone: Option<String>, interactive: bool) -> AppResult<String> {
    match zone {
        Some(zone) => {
            let zone = normalize_zone_name(&zone);
            let zones = runner.list_zones()?;
            if zones.contains(&zone) {
                Ok(zone)
            } else {
                Err(AppError::Message(format!(
                    "zone `{zone}` not found in PowerDNS"
                )))
            }
        }
        None if interactive => select_zone(runner),
        None => Err(AppError::Message(
            "missing `--zone` and stdin is not interactive".to_string(),
        )),
    }
}

fn select_zone(runner: &PdnsUtil) -> AppResult<String> {
    let zones = runner.list_zones()?;
    if zones.is_empty() {
        return Err(AppError::Message("no zones found".to_string()));
    }

    let index = prompt_select("Choose a zone", &zones)?;
    Ok(zones[index].clone())
}

fn prompt_record_type() -> AppResult<String> {
    let common = vec![
        "A".to_string(),
        "AAAA".to_string(),
        "CNAME".to_string(),
        "TXT".to_string(),
        "MX".to_string(),
        "NS".to_string(),
        "PTR".to_string(),
        "SRV".to_string(),
        "CAA".to_string(),
        "Custom".to_string(),
    ];

    let index = prompt_select("Choose a record type", &common)?;
    if common[index] == "Custom" {
        Ok(normalize_record_type(&prompt_input(
            "Custom record type",
            None,
        )?))
    } else {
        Ok(common[index].clone())
    }
}

fn prompt_optional_ttl() -> AppResult<Option<u32>> {
    let raw = prompt_input("TTL in seconds (blank = PowerDNS default)", Some(""))?;
    if raw.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(parse_ttl(&raw)?))
    }
}

fn prompt_content_for_type(record_type: &str, zone: &str) -> AppResult<String> {
    match record_type {
        "A" => prompt_input("IPv4 address", None),
        "AAAA" => prompt_input("IPv6 address", None),
        "CNAME" | "NS" | "PTR" => {
            let value = prompt_input("Target host", None)?;
            Ok(normalize_target_name(&value, zone))
        }
        "MX" => {
            let priority = prompt_input("MX priority", Some("10"))?;
            let target = prompt_input("MX target host", None)?;
            Ok(format!(
                "{} {}",
                priority.trim(),
                normalize_target_name(&target, zone)
            ))
        }
        "SRV" => {
            let priority = prompt_input("SRV priority", Some("10"))?;
            let weight = prompt_input("SRV weight", Some("10"))?;
            let port = prompt_input("SRV port", None)?;
            let target = prompt_input("SRV target host", None)?;
            Ok(format!(
                "{} {} {} {}",
                priority.trim(),
                weight.trim(),
                port.trim(),
                normalize_target_name(&target, zone)
            ))
        }
        "TXT" => {
            let text = prompt_input("TXT content", None)?;
            Ok(quote_txt_content(&text))
        }
        "CAA" => {
            let flags = prompt_input("CAA flags", Some("0"))?;
            let tag = prompt_input("CAA tag", Some("issue"))?;
            let value = prompt_input("CAA value", None)?;
            Ok(format!(
                "{} {} {}",
                flags.trim(),
                tag.trim(),
                quote_txt_content(&value)
            ))
        }
        _ => prompt_input("Raw content", None),
    }
}

fn select_record_for_delete(
    records: &[ZoneRecord],
    name: Option<&str>,
    record_type: Option<&str>,
    content: Option<&str>,
) -> AppResult<ZoneRecord> {
    let mut candidates = filter_records(records, name, record_type, content);

    if candidates.is_empty() {
        return Err(AppError::Message("no matching records found".to_string()));
    }

    if candidates.len() == 1 {
        return Ok(candidates.remove(0));
    }

    while candidates.len() > 30 {
        println!("Found {} candidate records.", candidates.len());
        let keyword = prompt_input("Filter by keyword (matches name/type/content)", Some(""))?;
        let keyword = keyword.trim().to_ascii_lowercase();

        if keyword.is_empty() {
            println!("Please narrow the result set.");
            continue;
        }

        candidates = candidates
            .into_iter()
            .filter(|record| {
                let line = format!("{} {} {}", record.name, record.record_type, record.content)
                    .to_ascii_lowercase();
                line.contains(&keyword)
            })
            .collect();

        if candidates.is_empty() {
            return Err(AppError::Message(
                "no matching records after filtering".to_string(),
            ));
        }
    }

    let labels: Vec<String> = candidates.iter().map(format_record_label).collect();
    let index = prompt_select("Choose the record to delete", &labels)?;
    Ok(candidates[index].clone())
}

fn find_record_exact(
    records: &[ZoneRecord],
    name: &str,
    record_type: &str,
    content: &str,
) -> AppResult<ZoneRecord> {
    let matches = filter_records(records, Some(name), Some(record_type), Some(content));

    if matches.is_empty() {
        Err(AppError::Message(format!(
            "record not found: {name} {record_type} {content}"
        )))
    } else {
        Ok(matches[0].clone())
    }
}

fn build_delete_plan(
    zone: &str,
    records: &[ZoneRecord],
    spec: &DeleteRecordSpec,
) -> AppResult<DeletePlan> {
    let matching_rrset: Vec<&ZoneRecord> = records
        .iter()
        .filter(|record| {
            record.name == spec.name && record.record_type.eq_ignore_ascii_case(&spec.record_type)
        })
        .collect();

    if matching_rrset.is_empty() {
        return Err(AppError::Message(format!(
            "rrset not found: {} {}",
            spec.name, spec.record_type
        )));
    }

    let ttl = matching_rrset.iter().find_map(|record| record.ttl);
    let mut removed = false;
    let mut remaining_contents = Vec::new();

    for record in matching_rrset {
        if !removed && record.content == spec.content {
            removed = true;
        } else {
            remaining_contents.push(record.content.clone());
        }
    }

    if !removed {
        return Err(AppError::Message(format!(
            "record content not found in rrset: {} {} {}",
            spec.name, spec.record_type, spec.content
        )));
    }

    let method = if remaining_contents.is_empty() {
        DeleteMethod::DeleteRrset
    } else {
        DeleteMethod::Replace {
            ttl,
            remaining_contents,
        }
    };

    Ok(DeletePlan {
        zone: zone.to_string(),
        name: spec.name.clone(),
        record_type: spec.record_type.clone(),
        method,
    })
}

fn filter_records(
    records: &[ZoneRecord],
    name: Option<&str>,
    record_type: Option<&str>,
    content: Option<&str>,
) -> Vec<ZoneRecord> {
    records
        .iter()
        .filter(|record| {
            let name_matches = name.map_or(true, |value| record.name == value);
            let type_matches =
                record_type.map_or(true, |value| record.record_type.eq_ignore_ascii_case(value));
            let content_matches = content.map_or(true, |value| record.content == value);
            name_matches && type_matches && content_matches
        })
        .cloned()
        .collect()
}

fn parse_zone_record_line(line: &str, current_origin: &str) -> Option<ZoneRecord> {
    let tokens = tokenize_dns_line(line);
    if tokens.len() < 3 {
        return None;
    }

    let name = normalize_owner_name(&tokens[0], current_origin);
    let mut index = 1;
    let mut ttl = None;

    if tokens.get(index).is_some_and(|value| looks_like_ttl(value)) {
        ttl = tokens[index].parse::<u32>().ok();
        index += 1;
    }

    if tokens.get(index).is_some_and(|value| is_dns_class(value)) {
        index += 1;
    }

    let record_type = tokens.get(index)?.to_string();
    index += 1;

    if index >= tokens.len() {
        return None;
    }

    Some(ZoneRecord {
        name,
        ttl,
        record_type: normalize_record_type(&record_type),
        content: tokens[index..].join(" "),
    })
}

fn parse_origin_directive(line: &str, current_origin: &str) -> Option<String> {
    let tokens = tokenize_dns_line(line);
    if tokens.len() >= 2 && tokens[0].eq_ignore_ascii_case("$ORIGIN") {
        Some(normalize_owner_name(&tokens[1], current_origin))
    } else {
        None
    }
}

fn print_help() {
    println!("ppdns - guided PowerDNS CLI");
    println!();
    println!("Usage:");
    println!("  ppdns");
    println!("  ppdns add record [--zone ZONE] [--name NAME] [--type TYPE] [--content CONTENT] [--ttl TTL] [-y]");
    println!(
        "  ppdns delete record [--zone ZONE] [--name NAME] [--type TYPE] [--content CONTENT] [-y]"
    );
    println!("  ppdns list zones");
    println!("  ppdns list records --zone ZONE");
    println!();
    println!("Global options:");
    println!("  --pdnsutil PATH");
    println!("  --config-dir DIR");
    println!("  --config-name NAME");
    println!("  --dry-run");
}

fn print_zones(zones: &[String]) -> AppResult<()> {
    if zones.is_empty() {
        println!("No zones found.");
        return Ok(());
    }

    println!("Zones:");
    for zone in zones {
        println!("  {zone}");
    }
    Ok(())
}

fn print_records(zone: &str, records: &[ZoneRecord]) -> AppResult<()> {
    println!("Zone: {zone}");

    if records.is_empty() {
        println!("No parseable records found.");
        return Ok(());
    }

    let name_width = records
        .iter()
        .map(|record| record.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let type_width = records
        .iter()
        .map(|record| record.record_type.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let ttl_width = records
        .iter()
        .map(|record| {
            record
                .ttl
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
                .len()
        })
        .max()
        .unwrap_or(3)
        .max(3);

    println!(
        "{:<name_width$}  {:<type_width$}  {:>ttl_width$}  CONTENT",
        "NAME",
        "TYPE",
        "TTL",
        name_width = name_width,
        type_width = type_width,
        ttl_width = ttl_width
    );

    for record in records {
        println!(
            "{:<name_width$}  {:<type_width$}  {:>ttl_width$}  {}",
            record.name,
            record.record_type,
            record
                .ttl
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            record.content,
            name_width = name_width,
            type_width = type_width,
            ttl_width = ttl_width
        );
    }

    Ok(())
}

fn detect_powerdns_status() -> PowerDnsStatus {
    if !command_exists("apt-cache") {
        return PowerDnsStatus::Unsupported {
            reason: "apt-based package manager not found".to_string(),
        };
    }

    let installed = detect_powerdns_installed_version().ok().flatten();
    let candidate = detect_apt_candidate_version("pdns-server").ok().flatten();

    match installed {
        Some(installed) => PowerDnsStatus::Installed {
            installed,
            candidate,
        },
        None => PowerDnsStatus::NotInstalled { candidate },
    }
}

fn detect_self_status() -> SelfStatus {
    let current = env!("CARGO_PKG_VERSION").to_string();

    match fetch_latest_ppdns_version() {
        Ok(latest) => {
            let update_available = compare_numeric_versions(&latest, &current).is_gt();
            SelfStatus::LatestKnown {
                current,
                latest,
                update_available,
            }
        }
        Err(err) => SelfStatus::UnknownLatest {
            current,
            reason: err.to_string(),
        },
    }
}

fn execute_powerdns_package_action(global: &GlobalOptions, action: PackageAction) -> AppResult<()> {
    if !command_exists("apt-get") || !command_exists("apt-cache") || !command_exists("install") {
        return Err(AppError::Message(
            "PowerDNS package management currently supports apt-based Linux only".to_string(),
        ));
    }

    let mut packages = detect_installed_powerdns_packages()?;

    if matches!(action, PackageAction::Install) || !packages.iter().any(|pkg| pkg == "pdns-server")
    {
        packages.push("pdns-server".to_string());
    }

    if !packages.iter().any(|pkg| pkg.starts_with("pdns-backend-")) {
        packages.push(prompt_powerdns_backend_package()?);
    }

    packages.sort();
    packages.dedup();

    let action_label = match action {
        PackageAction::Install => "install",
        PackageAction::Update => "update",
        PackageAction::Reinstall => "reinstall",
    };

    println!("Ready to {action_label} PowerDNS packages from current apt repositories:");
    for package in &packages {
        println!("  {package}");
    }

    if !prompt_confirm("Continue?", true)? {
        println!("Cancelled.");
        return Ok(());
    }

    run_system_status(global, "apt-get", &["update".to_string()], true)?;

    let mut args = vec!["install".to_string(), "-y".to_string()];
    match action {
        PackageAction::Install => {}
        PackageAction::Update => args.push("--only-upgrade".to_string()),
        PackageAction::Reinstall => args.push("--reinstall".to_string()),
    }
    args.extend(packages.iter().cloned());

    run_system_status(global, "apt-get", &args, true)?;
    println!("PowerDNS package action completed.");
    Ok(())
}

fn execute_self_update_action(
    global: &GlobalOptions,
    reinstall: bool,
    status: Option<&SelfStatus>,
) -> AppResult<()> {
    ensure_command_available("tar")?;

    let current = env!("CARGO_PKG_VERSION").to_string();
    let desired_version = if reinstall {
        current.clone()
    } else {
        match status {
            Some(SelfStatus::LatestKnown { latest, .. }) => latest.clone(),
            _ => fetch_latest_ppdns_version()?,
        }
    };

    let target = current_ppdns_target()?;
    let archive_name = format!("ppdns-{target}.tar.gz");
    let url = format!(
        "https://github.com/{}/releases/download/v{}/{}",
        PPDNS_RELEASE_REPO, desired_version, archive_name
    );
    let current_exe = env::current_exe()?;

    println!(
        "Ready to {} ppdns:",
        if reinstall { "reinstall" } else { "update" }
    );
    println!("  current: {current}");
    println!("  target:  {desired_version}");
    println!("  binary:  {}", current_exe.display());
    println!("  source:  {url}");

    if !prompt_confirm("Continue?", true)? {
        println!("Cancelled.");
        return Ok(());
    }

    let temp_dir = create_temp_workspace("ppdns-self-update")?;
    let archive_path = temp_dir.join(&archive_name);

    let result = (|| -> AppResult<()> {
        download_to_path(global, &url, &archive_path)?;

        run_system_status(
            global,
            "tar",
            &[
                "-xzf".to_string(),
                archive_path.display().to_string(),
                "-C".to_string(),
                temp_dir.display().to_string(),
            ],
            false,
        )?;

        let binary_path = find_file_named(&temp_dir, "ppdns")?.ok_or_else(|| {
            AppError::Message("could not find ppdns binary inside downloaded archive".to_string())
        })?;

        run_system_status(
            global,
            "install",
            &[
                "-m".to_string(),
                "0755".to_string(),
                binary_path.display().to_string(),
                current_exe.display().to_string(),
            ],
            true,
        )?;

        Ok(())
    })();

    let _ = fs::remove_dir_all(&temp_dir);
    result?;

    println!("ppdns {desired_version} installed.");
    Ok(())
}

fn detect_powerdns_installed_version() -> AppResult<Option<String>> {
    if command_exists("pdnsutil") {
        let output = run_external_capture("pdnsutil", &["--version".to_string()])?;
        if let Some(version) = extract_numeric_version(&output) {
            return Ok(Some(version));
        }
    }

    detect_dpkg_package_version("pdns-server")
}

fn detect_apt_candidate_version(package: &str) -> AppResult<Option<String>> {
    let output = run_external_capture("apt-cache", &["policy".to_string(), package.to_string()])?;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(candidate) = trimmed.strip_prefix("Candidate:") {
            let candidate = candidate.trim();
            if candidate == "(none)" {
                return Ok(None);
            }
            return Ok(Some(candidate.to_string()));
        }
    }

    Ok(None)
}

fn detect_dpkg_package_version(package: &str) -> AppResult<Option<String>> {
    if !command_exists("dpkg-query") {
        return Ok(None);
    }

    let output = run_shell_capture(&format!(
        "dpkg-query -W -f='${{Version}}\\n' {} 2>/dev/null || true",
        shell_quote(package)
    ))?;
    let version = output.trim();

    if version.is_empty() {
        Ok(None)
    } else {
        Ok(Some(version.to_string()))
    }
}

fn detect_installed_powerdns_packages() -> AppResult<Vec<String>> {
    if !command_exists("dpkg-query") {
        return Ok(Vec::new());
    }

    let output = run_shell_capture(
        "dpkg-query -W -f='${Package}\\n' 'pdns-server' 'pdns-tools' 'pdns-backend-*' 2>/dev/null || true",
    )?;
    let mut packages: Vec<String> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    packages.sort();
    packages.dedup();
    Ok(packages)
}

fn prompt_powerdns_backend_package() -> AppResult<String> {
    let labels = vec![
        "gsqlite3 (Recommended)".to_string(),
        "bind".to_string(),
        "gmysql".to_string(),
        "gpgsql".to_string(),
    ];
    let index = prompt_select("Choose a PowerDNS backend package", &labels)?;

    Ok(match index {
        0 => "pdns-backend-gsqlite3".to_string(),
        1 => "pdns-backend-bind".to_string(),
        2 => "pdns-backend-gmysql".to_string(),
        3 => "pdns-backend-gpgsql".to_string(),
        _ => unreachable!(),
    })
}

fn fetch_latest_ppdns_version() -> AppResult<String> {
    let body = fetch_url_text("https://api.github.com/repos/0x3st/ppdns/releases/latest")?;
    extract_json_string_field(&body, "tag_name")
        .map(|value| value.trim_start_matches('v').to_string())
        .ok_or_else(|| AppError::Message("could not parse latest ppdns release".to_string()))
}

fn fetch_url_text(url: &str) -> AppResult<String> {
    if command_exists("curl") {
        run_external_capture(
            "curl",
            &[
                "-fsSL".to_string(),
                "--connect-timeout".to_string(),
                "3".to_string(),
                "--max-time".to_string(),
                "5".to_string(),
                "-H".to_string(),
                "Accept: application/vnd.github+json".to_string(),
                "-H".to_string(),
                "User-Agent: ppdns".to_string(),
                url.to_string(),
            ],
        )
    } else if command_exists("wget") {
        run_external_capture(
            "wget",
            &[
                "-qO-".to_string(),
                "--timeout=5".to_string(),
                "--header=Accept: application/vnd.github+json".to_string(),
                "--header=User-Agent: ppdns".to_string(),
                url.to_string(),
            ],
        )
    } else {
        Err(AppError::Message(
            "curl or wget is required to check the latest ppdns release".to_string(),
        ))
    }
}

fn download_to_path(global: &GlobalOptions, url: &str, path: &Path) -> AppResult<()> {
    if command_exists("curl") {
        run_system_status(
            global,
            "curl",
            &[
                "-fL".to_string(),
                url.to_string(),
                "-o".to_string(),
                path.display().to_string(),
            ],
            false,
        )
    } else if command_exists("wget") {
        run_system_status(
            global,
            "wget",
            &[
                "-O".to_string(),
                path.display().to_string(),
                url.to_string(),
            ],
            false,
        )
    } else {
        Err(AppError::Message(
            "curl or wget is required to download release files".to_string(),
        ))
    }
}

fn current_ppdns_target() -> AppResult<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-musl"),
        _ => Err(AppError::Message(
            "self-update currently supports Linux x86_64 and aarch64 only".to_string(),
        )),
    }
}

fn compare_numeric_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let left_parts = parse_numeric_version_parts(left);
    let right_parts = parse_numeric_version_parts(right);
    let width = left_parts.len().max(right_parts.len());

    for index in 0..width {
        let left = *left_parts.get(index).unwrap_or(&0);
        let right = *right_parts.get(index).unwrap_or(&0);

        match left.cmp(&right) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }

    std::cmp::Ordering::Equal
}

fn parse_numeric_version_parts(value: &str) -> Vec<u32> {
    value
        .trim_start_matches('v')
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
        })
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

fn extract_numeric_version(value: &str) -> Option<String> {
    value
        .split_whitespace()
        .map(|token| token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '.'))
        .find(|token| {
            token.chars().any(|ch| ch == '.') && token.chars().any(|ch| ch.is_ascii_digit())
        })
        .map(ToOwned::to_owned)
}

fn extract_json_string_field(body: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{field}\":");
    let start = body.find(&pattern)?;
    let after = &body[start + pattern.len()..];
    let first_quote = after.find('"')?;
    let rest = &after[first_quote + 1..];
    let end_quote = rest.find('"')?;
    Some(rest[..end_quote].to_string())
}

fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", command))
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn current_user_is_root() -> AppResult<bool> {
    let output = run_external_capture("id", &["-u".to_string()])?;
    Ok(output.trim() == "0")
}

fn ensure_command_available(command: &str) -> AppResult<()> {
    if command_exists(command) {
        Ok(())
    } else {
        Err(AppError::Message(format!(
            "required command not found: `{command}`"
        )))
    }
}

fn run_external_capture(program: &str, args: &[String]) -> AppResult<String> {
    let output = Command::new(program).args(args).output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(AppError::CommandFailed {
            program: program.to_string(),
            args: args.to_vec(),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn run_shell_capture(script: &str) -> AppResult<String> {
    run_external_capture("sh", &["-c".to_string(), script.to_string()])
}

fn run_system_status(
    global: &GlobalOptions,
    program: &str,
    args: &[String],
    require_root: bool,
) -> AppResult<()> {
    let (runner, runner_args) = build_system_command(program, args, require_root)?;
    let preview = preview_external_command(&runner, &runner_args);

    if global.dry_run {
        println!("DRY RUN");
        println!("{preview}");
        return Ok(());
    }

    let output = Command::new(&runner).args(&runner_args).output()?;

    if output.status.success() {
        if !output.stdout.is_empty() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stdout = stdout.trim();
            if !stdout.is_empty() {
                println!("{stdout}");
            }
        }
        Ok(())
    } else {
        Err(AppError::CommandFailed {
            program: runner,
            args: runner_args,
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

fn build_system_command(
    program: &str,
    args: &[String],
    require_root: bool,
) -> AppResult<(String, Vec<String>)> {
    if require_root && !current_user_is_root()? {
        ensure_command_available("sudo")?;
        let mut runner_args = vec![program.to_string()];
        runner_args.extend(args.iter().cloned());
        Ok(("sudo".to_string(), runner_args))
    } else {
        Ok((program.to_string(), args.to_vec()))
    }
}

fn preview_external_command(program: &str, args: &[String]) -> String {
    let mut items = vec![program.to_string()];
    items.extend(args.iter().cloned());
    items
        .into_iter()
        .map(|item| shell_quote(&item))
        .collect::<Vec<_>>()
        .join(" ")
}

fn create_temp_workspace(prefix: &str) -> AppResult<PathBuf> {
    let dir = env::temp_dir().join(format!("{prefix}-{}", std::process::id()));
    if dir.exists() {
        let _ = fs::remove_dir_all(&dir);
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn find_file_named(root: &Path, name: &str) -> AppResult<Option<PathBuf>> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            if let Some(found) = find_file_named(&path, name)? {
                return Ok(Some(found));
            }
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

fn prompt_input(prompt: &str, default: Option<&str>) -> AppResult<String> {
    loop {
        match default {
            Some(default) if !default.is_empty() => print!("{prompt} [{default}]: "),
            Some(_) => print!("{prompt}: "),
            None => print!("{prompt}: "),
        }
        io::stdout().flush()?;

        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            return Err(AppError::Message("stdin closed".to_string()));
        }

        let line = line.trim().to_string();
        if line.is_empty() {
            if let Some(default) = default {
                return Ok(default.to_string());
            }
        } else {
            return Ok(line);
        }
    }
}

fn prompt_select(prompt: &str, items: &[String]) -> AppResult<usize> {
    if items.is_empty() {
        return Err(AppError::Message("selection list is empty".to_string()));
    }

    println!("{prompt}:");
    for (index, item) in items.iter().enumerate() {
        println!("  {}) {}", index + 1, item);
    }

    loop {
        let raw = prompt_input("Enter selection number", None)?;
        let number = match raw.parse::<usize>() {
            Ok(number) => number,
            Err(_) => {
                println!("Please enter a number.");
                continue;
            }
        };

        if (1..=items.len()).contains(&number) {
            return Ok(number - 1);
        }

        println!("Selection out of range.");
    }
}

fn prompt_confirm(prompt: &str, default: bool) -> AppResult<bool> {
    let suffix = if default { "[Y/n]" } else { "[y/N]" };

    loop {
        let value = prompt_input(&format!("{prompt} {suffix}"), Some(""))?;
        if value.trim().is_empty() {
            return Ok(default);
        }

        match value.to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("Please answer y or n."),
        }
    }
}

fn stdin_is_interactive() -> bool {
    io::stdin().is_terminal()
}

fn normalize_zone_name(input: &str) -> String {
    let trimmed = input.trim().trim_end_matches('.');
    format!("{trimmed}.")
}

fn normalize_owner_name(input: &str, zone: &str) -> String {
    let value = input.trim();
    if value.is_empty() || value == "@" {
        return normalize_zone_name(zone);
    }

    if value.ends_with('.') {
        return value.to_string();
    }

    let zone_without_dot = zone.trim_end_matches('.');
    if value.eq_ignore_ascii_case(zone_without_dot)
        || value
            .to_ascii_lowercase()
            .ends_with(&format!(".{}", zone_without_dot.to_ascii_lowercase()))
    {
        return format!("{value}.");
    }

    if value.contains('.') {
        return format!("{value}.");
    }

    format!("{value}.{zone_without_dot}.")
}

fn normalize_target_name(input: &str, zone: &str) -> String {
    normalize_owner_name(input, zone)
}

fn normalize_record_type(input: &str) -> String {
    input.trim().to_ascii_uppercase()
}

fn parse_ttl(value: &str) -> AppResult<u32> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| AppError::Message(format!("invalid TTL: `{value}`")))
}

fn looks_like_ttl(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit())
}

fn is_dns_class(value: &str) -> bool {
    matches!(value.to_ascii_uppercase().as_str(), "IN" | "CH" | "HS")
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '='))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', r#"'\''"#))
    }
}

fn strip_comment_preserving_quotes(line: &str) -> String {
    let mut result = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in line.chars() {
        if escaped {
            result.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                result.push(ch);
                escaped = true;
            }
            '"' => {
                result.push(ch);
                in_quotes = !in_quotes;
            }
            ';' if !in_quotes => break,
            _ => result.push(ch),
        }
    }

    result.trim().to_string()
}

fn tokenize_dns_line(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in line.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                current.push(ch);
                escaped = true;
            }
            '"' => {
                current.push(ch);
                in_quotes = !in_quotes;
            }
            ch if ch.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn quote_txt_content(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        trimmed.to_string()
    } else {
        format!("\"{}\"", trimmed.replace('\\', r"\\").replace('"', r#"\""#))
    }
}

fn format_record_label(record: &ZoneRecord) -> String {
    format!(
        "{}  {:<5}  {:<6}  {}",
        record.name,
        record.record_type,
        record
            .ttl
            .map(|ttl| ttl.to_string())
            .unwrap_or_else(|| "-".to_string()),
        record.content
    )
}

fn expect_value(cursor: &mut ArgCursor, flag: &str) -> AppResult<String> {
    cursor
        .next()
        .ok_or_else(|| AppError::Message(format!("missing value for `{flag}`")))
}

fn is_help(value: &str) -> bool {
    matches!(value, "-h" | "--help" | "help")
}

fn detect_pdns_syntax(help_output: &str) -> PdnsSyntax {
    let normalized = help_output.to_ascii_lowercase();

    if normalized.contains("zone list-all") || normalized.contains("rrset add") {
        PdnsSyntax::Modern
    } else if normalized.contains("list-all-zones")
        || normalized.contains("add-record")
        || normalized.contains("replace-rrset")
    {
        PdnsSyntax::Legacy
    } else {
        PdnsSyntax::Modern
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_names_are_normalized() {
        assert_eq!(normalize_zone_name("example.com"), "example.com.");
        assert_eq!(normalize_zone_name("example.com."), "example.com.");
    }

    #[test]
    fn owner_names_are_expanded_for_zone() {
        assert_eq!(
            normalize_owner_name("www", "example.com."),
            "www.example.com."
        );
        assert_eq!(normalize_owner_name("@", "example.com."), "example.com.");
        assert_eq!(
            normalize_owner_name("www.example.com", "example.com."),
            "www.example.com."
        );
    }

    #[test]
    fn txt_values_are_quoted() {
        assert_eq!(quote_txt_content("hello"), "\"hello\"");
        assert_eq!(quote_txt_content("\"hello\""), "\"hello\"");
    }

    #[test]
    fn zone_lines_are_parsed() {
        let record = parse_zone_record_line("www 300 IN A 1.2.3.4", "example.com.")
            .expect("record should parse");

        assert_eq!(record.name, "www.example.com.");
        assert_eq!(record.ttl, Some(300));
        assert_eq!(record.record_type, "A");
        assert_eq!(record.content, "1.2.3.4");
    }

    #[test]
    fn txt_zone_lines_keep_spaces() {
        let record = parse_zone_record_line("txt 300 IN TXT \"hello world\"", "example.com.")
            .expect("record should parse");

        assert_eq!(record.content, "\"hello world\"");
    }

    #[test]
    fn delete_plan_replaces_multi_value_rrset() {
        let records = vec![
            ZoneRecord {
                name: "www.example.com.".to_string(),
                ttl: Some(300),
                record_type: "A".to_string(),
                content: "1.1.1.1".to_string(),
            },
            ZoneRecord {
                name: "www.example.com.".to_string(),
                ttl: Some(300),
                record_type: "A".to_string(),
                content: "2.2.2.2".to_string(),
            },
        ];

        let spec = DeleteRecordSpec {
            zone: "example.com.".to_string(),
            name: "www.example.com.".to_string(),
            record_type: "A".to_string(),
            content: "1.1.1.1".to_string(),
        };

        let plan =
            build_delete_plan("example.com.", &records, &spec).expect("delete plan should build");

        match plan.method {
            DeleteMethod::Replace {
                ttl,
                remaining_contents,
            } => {
                assert_eq!(ttl, Some(300));
                assert_eq!(remaining_contents, vec!["2.2.2.2".to_string()]);
            }
            DeleteMethod::DeleteRrset => panic!("expected replace plan"),
        }
    }

    #[test]
    fn delete_plan_deletes_last_value() {
        let records = vec![ZoneRecord {
            name: "www.example.com.".to_string(),
            ttl: Some(300),
            record_type: "A".to_string(),
            content: "1.1.1.1".to_string(),
        }];

        let spec = DeleteRecordSpec {
            zone: "example.com.".to_string(),
            name: "www.example.com.".to_string(),
            record_type: "A".to_string(),
            content: "1.1.1.1".to_string(),
        };

        let plan =
            build_delete_plan("example.com.", &records, &spec).expect("delete plan should build");

        assert!(matches!(plan.method, DeleteMethod::DeleteRrset));
    }

    #[test]
    fn detects_legacy_pdnsutil_syntax() {
        let help = "Commands:\nlist-all-zones\nlist-zone ZONE\nadd-record ZONE NAME TYPE";
        assert_eq!(detect_pdns_syntax(help), PdnsSyntax::Legacy);
    }

    #[test]
    fn detects_modern_pdnsutil_syntax() {
        let help = "Commands:\nzone list-all\nzone list ZONE\nrrset add ZONE NAME TYPE";
        assert_eq!(detect_pdns_syntax(help), PdnsSyntax::Modern);
    }

    #[test]
    fn legacy_add_command_uses_old_pdnsutil_form() {
        let runner = PdnsUtil {
            global: GlobalOptions::default(),
            syntax: PdnsSyntax::Legacy,
        };
        let spec = AddRecordSpec {
            zone: "example.com.".to_string(),
            name: "www.example.com.".to_string(),
            record_type: "A".to_string(),
            content: "1.2.3.4".to_string(),
            ttl: Some(300),
        };

        assert_eq!(
            runner.add_record_args(&spec),
            vec![
                "add-record".to_string(),
                "example.com.".to_string(),
                "www.example.com.".to_string(),
                "A".to_string(),
                "300".to_string(),
                "1.2.3.4".to_string()
            ]
        );
    }

    #[test]
    fn legacy_delete_replace_uses_old_pdnsutil_form() {
        let runner = PdnsUtil {
            global: GlobalOptions::default(),
            syntax: PdnsSyntax::Legacy,
        };
        let plan = DeletePlan {
            zone: "example.com.".to_string(),
            name: "www.example.com.".to_string(),
            record_type: "A".to_string(),
            method: DeleteMethod::Replace {
                ttl: Some(300),
                remaining_contents: vec!["5.6.7.8".to_string()],
            },
        };

        assert_eq!(
            runner.delete_plan_args(&plan),
            vec![
                "replace-rrset".to_string(),
                "example.com.".to_string(),
                "www.example.com.".to_string(),
                "A".to_string(),
                "300".to_string(),
                "5.6.7.8".to_string()
            ]
        );
    }

    #[test]
    fn version_comparison_detects_newer_patch_release() {
        assert!(compare_numeric_versions("1.0.2", "1.0.1").is_gt());
        assert!(compare_numeric_versions("1.0.1", "1.0.1").is_eq());
        assert!(compare_numeric_versions("1.0.0", "1.0.1").is_lt());
    }

    #[test]
    fn home_actions_include_update_paths_when_available() {
        let status = HomeStatus {
            powerdns: PowerDnsStatus::Installed {
                installed: "4.8.3".to_string(),
                candidate: Some("5.0.0".to_string()),
            },
            ppdns: SelfStatus::LatestKnown {
                current: "1.0.0".to_string(),
                latest: "1.0.1".to_string(),
                update_available: true,
            },
        };

        let actions = build_home_actions(&status);
        let labels: Vec<String> = actions.into_iter().map(|(label, _)| label).collect();

        assert!(labels.contains(&"Update PowerDNS".to_string()));
        assert!(labels.contains(&"Reinstall PowerDNS".to_string()));
        assert!(labels.contains(&"Update ppdns".to_string()));
        assert!(labels.contains(&"Reinstall ppdns".to_string()));
    }
}
