use crate::config::{load_config, save_config, workbench_home, DbServerProfile};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
pub struct DbTransferProgressEvent {
    pub op: String,     // "export" | "import"
    pub target: String, // db name or server name
    pub stage: String,  // "running" | "done" | "error"
    pub bytes_processed: u64,
    pub total_bytes: u64,
    pub percent: u8,
    pub file_path: Option<String>,
    pub message: String,
}

const SYSTEM_SCHEMAS: &[&str] = &["information_schema", "mysql", "performance_schema", "sys"];
const PROTECTED_USERS: &[&str] = &[
    "root",
    "PUBLIC",
    "mariadb.sys",
    "mysql.sys",
    "mysql.session",
    "mysql.infoschema",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseInfo {
    pub name: String,
    pub table_count: u64,
    pub size_bytes: u64,
    pub charset: String,
    pub collation: String,
    pub is_system: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbUserGrant {
    pub database: String,
    pub privilege: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbUserInfo {
    pub username: String,
    pub host: String,
    pub is_protected: bool,
    pub grants: Vec<DbUserGrant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbServerInspection {
    pub server: DbServerProfile,
    pub connected: bool,
    pub server_version: Option<String>,
    pub error: Option<String>,
    pub databases: Vec<DatabaseInfo>,
    pub users: Vec<DbUserInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTableInfo {
    pub name: String,
    pub table_type: String,
    pub engine: String,
    pub row_count: u64,
    pub size_bytes: u64,
    pub collation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbColumnInfo {
    pub name: String,
    pub col_type: String,
    pub nullable: bool,
    pub key: String,
    pub default_value: Option<String>,
    pub extra: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTablePreview {
    pub table: String,
    pub columns: Vec<DbColumnInfo>,
    pub rows: Vec<Vec<String>>,
    pub total_rows: u64,
    pub limit: u32,
    pub offset: u64,
}

pub fn validate_identifier(raw: &str, label: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(format!("{} must be between 1 and 64 characters.", label));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '$')
    {
        return Err(format!(
            "{} contains invalid characters (only letters, numbers, _, -, $ allowed).",
            label
        ));
    }
    Ok(trimmed.to_string())
}

pub fn validate_db_host(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 255 {
        return Err("Host must be between 1 and 255 characters.".to_string());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '%' | ':' | '/'))
    {
        return Err(
            "Host contains invalid characters (only letters, numbers, ., _, -, %, :, / allowed)."
                .to_string(),
        );
    }
    Ok(trimmed.to_string())
}

pub fn escape_sql_str(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('\'', "''")
}

struct TempMysqlDefaultsFile {
    path: PathBuf,
}

impl TempMysqlDefaultsFile {
    fn create(password: &str) -> Result<Self, String> {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let dir = workbench_home().join("data");
        fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed creating data dir for DB client config: {}", e))?;

        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = dir.join(format!(".my-{}-{}.cnf", pid, unique));

        let escaped = password
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace(['\n', '\r'], "");
        let content = format!("[client]\npassword=\"{}\"\n", escaped);

        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .map_err(|e| format!("Failed creating temp DB defaults file: {}", e))?;
            f.write_all(content.as_bytes())
                .map_err(|e| format!("Failed writing temp DB defaults file: {}", e))?;
        }
        #[cfg(not(unix))]
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| format!("Failed creating temp DB defaults file: {}", e))?;
            f.write_all(content.as_bytes())
                .map_err(|e| format!("Failed writing temp DB defaults file: {}", e))?;
        }

        Ok(Self { path })
    }

    fn arg(&self) -> String {
        format!("--defaults-extra-file={}", self.path.display())
    }
}

impl Drop for TempMysqlDefaultsFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn find_cli_binary(managed_names: &[&str], host_names: &[&str]) -> Option<PathBuf> {
    // 1. Check Workbench-managed MariaDB installs first
    let mariadb_root = workbench_home().join("runtimes/services/mariadb");
    if let Ok(entries) = fs::read_dir(&mariadb_root) {
        for entry in entries.flatten() {
            if !entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                continue;
            }
            for name in managed_names {
                let candidate = entry.path().join("bin").join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    // 2. Fallback to expanded host search (PATH + Homebrew + ServBay + standard dirs)
    crate::shell::find_binary_anywhere(host_names)
}

fn find_mysql_client() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let managed = &["mariadb.exe", "mysql.exe"];
    #[cfg(not(target_os = "windows"))]
    let managed = &["mariadb", "mysql"];

    find_cli_binary(managed, &["mariadb", "mysql"]).ok_or_else(|| {
        "No mariadb/mysql CLI client found. Install MariaDB in Services or via Homebrew/apt."
            .to_string()
    })
}

fn find_mysqldump_client() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    let managed = &["mariadb-dump.exe", "mysqldump.exe"];
    #[cfg(not(target_os = "windows"))]
    let managed = &["mariadb-dump", "mysqldump"];

    find_cli_binary(managed, &["mariadb-dump", "mysqldump"]).ok_or_else(|| {
        "No mariadb-dump/mysqldump CLI found. Install MariaDB in Services or via Homebrew/apt."
            .to_string()
    })
}

fn build_base_args(server: &DbServerProfile) -> Vec<String> {
    vec![
        format!("--host={}", server.host),
        format!("--port={}", server.port),
        format!("--user={}", server.username),
        "--protocol=TCP".to_string(),
        "--connect-timeout=10".to_string(),
    ]
}

fn run_sql_stdin(server: &DbServerProfile, sql: &str) -> Result<String, String> {
    let bin = find_mysql_client()?;
    let _creds_file = if !server.password.is_empty() {
        Some(TempMysqlDefaultsFile::create(&server.password)?)
    } else {
        None
    };

    let mut cmd = Command::new(bin);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    if let Some(ref cf) = _creds_file {
        cmd.arg(cf.arg());
    }
    cmd.args(build_base_args(server))
        .args(["--batch", "--skip-column-names"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed spawning SQL client: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(sql.as_bytes())
            .map_err(|e| format!("Failed writing SQL statement: {}", e))?;
    }

    let out = child
        .wait_with_output()
        .map_err(|e| format!("Failed waiting for SQL client: {}", e))?;

    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() {
            format!("SQL command exited with status {}", out.status)
        } else {
            err
        })
    }
}

fn run_sql_query(server: &DbServerProfile, sql: &str) -> Result<String, String> {
    let bin = find_mysql_client()?;
    let _creds_file = if !server.password.is_empty() {
        Some(TempMysqlDefaultsFile::create(&server.password)?)
    } else {
        None
    };

    let mut cmd = Command::new(bin);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    if let Some(ref cf) = _creds_file {
        cmd.arg(cf.arg());
    }
    cmd.args(build_base_args(server))
        .args(["--batch", "--skip-column-names", "-e", sql]);

    let out = cmd
        .output()
        .map_err(|e| format!("Failed executing SQL client: {}", e))?;

    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() {
            format!("SQL command exited with status {}", out.status)
        } else {
            err
        })
    }
}

pub fn get_server_by_id(server_id: &str) -> Result<DbServerProfile, String> {
    let cfg = load_config()?;
    cfg.db_servers
        .into_iter()
        .find(|s| s.id == server_id)
        .ok_or_else(|| format!("Database server '{}' not found", server_id))
}

pub fn save_db_server(mut profile: DbServerProfile) -> Result<Vec<DbServerProfile>, String> {
    profile.id = validate_identifier(&profile.id, "Server ID")?;
    profile.host = validate_db_host(&profile.host)?;
    let engine = profile.engine.trim().to_ascii_lowercase();
    if !matches!(engine.as_str(), "mariadb" | "mysql" | "postgresql" | "redis") {
        return Err(format!("Unsupported database engine: {}", profile.engine));
    }
    profile.engine = engine;
    if profile.name.trim().is_empty() || profile.name.len() > 80 {
        return Err("Server name must be between 1 and 80 characters.".to_string());
    }
    profile.name = profile.name.trim().to_string();
    if profile.port == 0 {
        return Err("Port must be between 1 and 65535.".to_string());
    }

    let mut cfg = load_config()?;
    if let Some(existing) = cfg.db_servers.iter_mut().find(|s| s.id == profile.id) {
        *existing = profile;
    } else {
        cfg.db_servers.push(profile);
    }
    save_config(&cfg)?;
    Ok(cfg.db_servers)
}

pub fn remove_db_server(server_id: &str) -> Result<Vec<DbServerProfile>, String> {
    let mut cfg = load_config()?;
    cfg.db_servers.retain(|s| s.id != server_id);
    save_config(&cfg)?;
    Ok(cfg.db_servers)
}

pub fn inspect_db_server(server_id: &str) -> Result<DbServerInspection, String> {
    let server = get_server_by_id(server_id)?;

    let version = match run_sql_query(&server, "SELECT VERSION();") {
        Ok(v) => v.lines().next().map(|l| l.trim().to_string()),
        Err(err) => {
            return Ok(DbServerInspection {
                server,
                connected: false,
                server_version: None,
                error: Some(err),
                databases: vec![],
                users: vec![],
            });
        }
    };

    let db_sql = "SELECT s.SCHEMA_NAME, COALESCE(COUNT(t.TABLE_NAME), 0), COALESCE(SUM(t.DATA_LENGTH + t.INDEX_LENGTH), 0), s.DEFAULT_CHARACTER_SET_NAME, s.DEFAULT_COLLATION_NAME FROM information_schema.SCHEMATA s LEFT JOIN information_schema.TABLES t ON s.SCHEMA_NAME = t.TABLE_SCHEMA GROUP BY s.SCHEMA_NAME, s.DEFAULT_CHARACTER_SET_NAME, s.DEFAULT_COLLATION_NAME ORDER BY s.SCHEMA_NAME;";
    let db_raw = run_sql_query(&server, db_sql).unwrap_or_default();
    let databases: Vec<DatabaseInfo> = db_raw
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 5 {
                return None;
            }
            let name = parts[0].trim().to_string();
            let is_system = SYSTEM_SCHEMAS.contains(&name.as_str());
            Some(DatabaseInfo {
                name,
                table_count: parts[1].trim().parse().unwrap_or(0),
                size_bytes: parts[2].trim().parse().unwrap_or(0),
                charset: parts[3].trim().to_string(),
                collation: parts[4].trim().to_string(),
                is_system,
            })
        })
        .collect();

    // Fetch users & per-database grants
    let grants_sql = "SELECT User, Host, Db, Select_priv, Insert_priv, Alter_priv FROM mysql.db;";
    let mut grants_map: HashMap<(String, String), Vec<DbUserGrant>> = HashMap::new();
    if let Ok(grants_raw) = run_sql_query(&server, grants_sql) {
        for line in grants_raw.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 6 {
                continue;
            }
            let key = (parts[0].trim().to_string(), parts[1].trim().to_string());
            let db_name = parts[2].trim().to_string();
            let is_full = parts[4].trim() == "Y" || parts[5].trim() == "Y";
            let privilege = if is_full {
                "ALL PRIVILEGES".to_string()
            } else {
                "READ ONLY".to_string()
            };
            grants_map.entry(key).or_default().push(DbUserGrant {
                database: db_name,
                privilege,
            });
        }
    }

    let users_sql = "SELECT User, Host FROM mysql.user ORDER BY User, Host;";
    let users_raw = run_sql_query(&server, users_sql).unwrap_or_default();
    let users: Vec<DbUserInfo> = users_raw
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 2 {
                return None;
            }
            let username = parts[0].trim().to_string();
            if username.is_empty() {
                return None;
            }
            let host = parts[1].trim().to_string();
            let is_protected = PROTECTED_USERS.contains(&username.as_str());
            let grants = grants_map
                .remove(&(username.clone(), host.clone()))
                .unwrap_or_default();
            Some(DbUserInfo {
                username,
                host,
                is_protected,
                grants,
            })
        })
        .collect();

    Ok(DbServerInspection {
        server,
        connected: true,
        server_version: version,
        error: None,
        databases,
        users,
    })
}

pub fn create_database(
    server_id: &str,
    name: &str,
    charset: Option<&str>,
    collation: Option<&str>,
) -> Result<(), String> {
    let server = get_server_by_id(server_id)?;
    let clean_name = validate_identifier(name, "Database name")?;
    let cs = validate_identifier(charset.unwrap_or("utf8mb4"), "Charset")?;
    let col = validate_identifier(collation.unwrap_or("utf8mb4_unicode_ci"), "Collation")?;
    let sql = format!(
        "CREATE DATABASE `{}` CHARACTER SET {} COLLATE {};",
        clean_name, cs, col
    );
    run_sql_query(&server, &sql)?;
    Ok(())
}

pub fn drop_database(server_id: &str, name: &str) -> Result<(), String> {
    let server = get_server_by_id(server_id)?;
    let clean_name = validate_identifier(name, "Database name")?;
    if SYSTEM_SCHEMAS.contains(&clean_name.as_str()) {
        return Err(format!("Refusing to drop system database '{}'", clean_name));
    }
    let sql = format!("DROP DATABASE `{}`;", clean_name);
    run_sql_query(&server, &sql)?;
    Ok(())
}

pub fn create_db_user(
    server_id: &str,
    username: &str,
    host: &str,
    password: &str,
) -> Result<(), String> {
    let server = get_server_by_id(server_id)?;
    let clean_user = validate_identifier(username, "Username")?;
    let clean_host = validate_db_host(host)?;
    let sql = format!(
        "CREATE USER '{}'@'{}' IDENTIFIED BY '{}'; FLUSH PRIVILEGES;\n",
        escape_sql_str(&clean_user),
        escape_sql_str(&clean_host),
        escape_sql_str(password)
    );
    // Pipe via stdin so the new user's password never appears in process argv
    run_sql_stdin(&server, &sql)?;
    Ok(())
}

pub fn drop_db_user(server_id: &str, username: &str, host: &str) -> Result<(), String> {
    let server = get_server_by_id(server_id)?;
    let clean_user = validate_identifier(username, "Username")?;
    let clean_host = validate_db_host(host)?;
    if PROTECTED_USERS.contains(&clean_user.as_str()) {
        return Err(format!("Refusing to drop protected user '{}'", clean_user));
    }
    let sql = format!(
        "DROP USER '{}'@'{}'; FLUSH PRIVILEGES;",
        escape_sql_str(&clean_user),
        escape_sql_str(&clean_host)
    );
    run_sql_query(&server, &sql)?;
    Ok(())
}

pub fn assign_user_to_database(
    server_id: &str,
    username: &str,
    host: &str,
    database: &str,
    read_only: bool,
) -> Result<(), String> {
    let server = get_server_by_id(server_id)?;
    let clean_user = validate_identifier(username, "Username")?;
    let clean_host = validate_db_host(host)?;
    let clean_db = validate_identifier(database, "Database name")?;
    let privs = if read_only { "SELECT" } else { "ALL PRIVILEGES" };
    let sql = format!(
        "GRANT {} ON `{}`.* TO '{}'@'{}'; FLUSH PRIVILEGES;",
        privs,
        clean_db,
        escape_sql_str(&clean_user),
        escape_sql_str(&clean_host)
    );
    run_sql_query(&server, &sql)?;
    Ok(())
}

pub fn revoke_user_from_database(
    server_id: &str,
    username: &str,
    host: &str,
    database: &str,
) -> Result<(), String> {
    let server = get_server_by_id(server_id)?;
    let clean_user = validate_identifier(username, "Username")?;
    let clean_host = validate_db_host(host)?;
    let clean_db = validate_identifier(database, "Database name")?;
    let sql = format!(
        "REVOKE ALL PRIVILEGES ON `{}`.* FROM '{}'@'{}'; FLUSH PRIVILEGES;",
        clean_db,
        escape_sql_str(&clean_user),
        escape_sql_str(&clean_host)
    );
    run_sql_query(&server, &sql)?;
    Ok(())
}

/// Strips `DEFINER=` clauses from mysqldump/mariadb-dump SQL lines so backups
/// can be imported cleanly under any local or remote database user.
pub fn strip_sql_definer_line(line: &str) -> String {
    if !line.contains("DEFINER=") {
        return line.to_string();
    }
    let mut s = line.to_string();

    // 1. Strip conditional comment wrapper: /*!50013 DEFINER=`user`@`host` SQL SECURITY DEFINER */
    while let Some(start) = s.find("/*!50013 DEFINER=") {
        if let Some(end_rel) = s[start..].find("*/") {
            let end = start + end_rel + 2;
            s.replace_range(start..end, "");
        } else {
            break;
        }
    }

    // 2. Strip inline `DEFINER=`user`@`host` ` or `DEFINER='user'@'host' `
    while let Some(start) = s.find("DEFINER=") {
        let rest = &s[start + "DEFINER=".len()..];
        let mut end_offset = 0usize;
        let bytes = rest.as_bytes();
        let mut in_quote: Option<u8> = None;
        for (i, &b) in bytes.iter().enumerate() {
            match in_quote {
                Some(q) => {
                    if b == q {
                        in_quote = None;
                    }
                    end_offset = i + 1;
                }
                None => {
                    if b == b'`' || b == b'\'' || b == b'"' {
                        in_quote = Some(b);
                        end_offset = i + 1;
                    } else if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
                        end_offset = i + 1;
                        break;
                    } else if b == b'*' && bytes.get(i + 1) == Some(&b'/') {
                        break;
                    } else {
                        end_offset = i + 1;
                    }
                }
            }
        }
        s.replace_range(start..start + "DEFINER=".len() + end_offset, "");
    }

    s
}

struct ProgressReader<R> {
    inner: R,
    read_bytes: u64,
    last_emitted: u64,
    total_bytes: u64,
    app: AppHandle,
    target: String,
    file_path: String,
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.read_bytes += n as u64;
            if self.read_bytes.saturating_sub(self.last_emitted) >= 262_144
                || self.read_bytes >= self.total_bytes
            {
                self.last_emitted = self.read_bytes;
                let pct = self
                    .read_bytes
                    .saturating_mul(100)
                    .checked_div(self.total_bytes)
                    .map(|p| p.min(99) as u8)
                    .unwrap_or(0);
                let _ = self.app.emit(
                    "db-transfer-progress",
                    DbTransferProgressEvent {
                        op: "import".to_string(),
                        target: self.target.clone(),
                        stage: "running".to_string(),
                        bytes_processed: self.read_bytes,
                        total_bytes: self.total_bytes,
                        percent: pct,
                        file_path: Some(self.file_path.clone()),
                        message: format!("Importing into {}...", self.target),
                    },
                );
            }
        }
        Ok(n)
    }
}

fn run_dump_to_file(
    app: &AppHandle,
    server: &DbServerProfile,
    target_label: &str,
    extra_args: &[String],
    output_path: &str,
) -> Result<u64, String> {
    let emit_err = |msg: &str| {
        let _ = app.emit(
            "db-transfer-progress",
            DbTransferProgressEvent {
                op: "export".to_string(),
                target: target_label.to_string(),
                stage: "error".to_string(),
                bytes_processed: 0,
                total_bytes: 0,
                percent: 0,
                file_path: Some(output_path.to_string()),
                message: msg.to_string(),
            },
        );
    };

    let _ = app.emit(
        "db-transfer-progress",
        DbTransferProgressEvent {
            op: "export".to_string(),
            target: target_label.to_string(),
            stage: "running".to_string(),
            bytes_processed: 0,
            total_bytes: 0,
            percent: 0,
            file_path: Some(output_path.to_string()),
            message: format!("Exporting {}...", target_label),
        },
    );

    let bin = match find_mysqldump_client() {
        Ok(b) => b,
        Err(e) => {
            emit_err(&e);
            return Err(e);
        }
    };
    let out_file = match File::create(output_path) {
        Ok(f) => f,
        Err(e) => {
            let msg = format!("Failed creating export file {}: {}", output_path, e);
            emit_err(&msg);
            return Err(msg);
        }
    };

    let _creds_file = if !server.password.is_empty() {
        match TempMysqlDefaultsFile::create(&server.password) {
            Ok(cf) => Some(cf),
            Err(e) => {
                emit_err(&e);
                return Err(e);
            }
        }
    } else {
        None
    };

    let mut cmd = Command::new(bin);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    if let Some(ref cf) = _creds_file {
        cmd.arg(cf.arg());
    }
    cmd.args(build_base_args(server))
        .args([
            "--single-transaction",
            "--quick",
            "--force",
            "--dump-date",
            "--routines",
            "--triggers",
            "--events",
            "--add-drop-table",
            "--add-drop-trigger",
            "--hex-blob",
            "--default-character-set=utf8mb4",
        ])
        .args(extra_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("Failed running dump client: {}", e);
            emit_err(&msg);
            return Err(msg);
        }
    };

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed capturing dump stdout".to_string())?;
    let stderr = child.stderr.take();

    // Drain stderr concurrently on a background thread so it can never deadlock
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut se) = stderr {
            let _ = se.read_to_string(&mut buf);
        }
        buf
    });

    let use_gzip = output_path.to_ascii_lowercase().ends_with(".gz");
    let mut streamed_bytes: u64 = 0;
    let mut last_emitted: u64 = 0;
    let mut saw_dump_completed = false;

    let write_res: Result<(), String> = (|| {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        if use_gzip {
            let mut encoder = GzEncoder::new(BufWriter::new(out_file), Compression::default());
            loop {
                line.clear();
                let n = reader
                    .read_line(&mut line)
                    .map_err(|e| format!("Failed reading dump stream: {}", e))?;
                if n == 0 {
                    break;
                }
                if line.starts_with("-- Dump completed") {
                    saw_dump_completed = true;
                }
                let cleaned = strip_sql_definer_line(&line);
                encoder
                    .write_all(cleaned.as_bytes())
                    .map_err(|e| format!("Failed writing gzipped dump: {}", e))?;
                streamed_bytes += cleaned.len() as u64;
                if streamed_bytes.saturating_sub(last_emitted) >= 262_144 {
                    last_emitted = streamed_bytes;
                    let _ = app.emit(
                        "db-transfer-progress",
                        DbTransferProgressEvent {
                            op: "export".to_string(),
                            target: target_label.to_string(),
                            stage: "running".to_string(),
                            bytes_processed: streamed_bytes,
                            total_bytes: 0,
                            percent: 0,
                            file_path: Some(output_path.to_string()),
                            message: format!("Exporting {}...", target_label),
                        },
                    );
                }
            }
            encoder
                .finish()
                .map_err(|e| format!("Failed finalizing gzipped dump: {}", e))?;
        } else {
            let mut writer = BufWriter::new(out_file);
            loop {
                line.clear();
                let n = reader
                    .read_line(&mut line)
                    .map_err(|e| format!("Failed reading dump stream: {}", e))?;
                if n == 0 {
                    break;
                }
                if line.starts_with("-- Dump completed") {
                    saw_dump_completed = true;
                }
                let cleaned = strip_sql_definer_line(&line);
                writer
                    .write_all(cleaned.as_bytes())
                    .map_err(|e| format!("Failed writing SQL dump: {}", e))?;
                streamed_bytes += cleaned.len() as u64;
                if streamed_bytes.saturating_sub(last_emitted) >= 262_144 {
                    last_emitted = streamed_bytes;
                    let _ = app.emit(
                        "db-transfer-progress",
                        DbTransferProgressEvent {
                            op: "export".to_string(),
                            target: target_label.to_string(),
                            stage: "running".to_string(),
                            bytes_processed: streamed_bytes,
                            total_bytes: 0,
                            percent: 0,
                            file_path: Some(output_path.to_string()),
                            message: format!("Exporting {}...", target_label),
                        },
                    );
                }
            }
            writer
                .flush()
                .map_err(|e| format!("Failed flushing SQL dump: {}", e))?;
        }
        Ok(())
    })();

    let status = child
        .wait()
        .map_err(|e| format!("Failed waiting for dump client: {}", e))?;
    let stderr_text = stderr_handle.join().unwrap_or_default().trim().to_string();

    if let Err(e) = write_res {
        let _ = fs::remove_file(output_path);
        emit_err(&e);
        return Err(e);
    }

    let bytes = Path::new(output_path)
        .metadata()
        .map(|m| m.len())
        .unwrap_or(0);

    if !status.success() {
        // With --force, mariadb-dump skips broken views/objects, logs them to stderr,
        // finishes the remaining databases, and writes "-- Dump completed".
        if saw_dump_completed && streamed_bytes > 0 && bytes > 0 {
            let warn_msg = if stderr_text.is_empty() {
                format!(
                    "Exported {} with warnings (exit status {}).",
                    target_label, status
                )
            } else {
                stderr_text
            };
            let _ = app.emit(
                "db-transfer-progress",
                DbTransferProgressEvent {
                    op: "export".to_string(),
                    target: target_label.to_string(),
                    stage: "warning".to_string(),
                    bytes_processed: bytes,
                    total_bytes: bytes,
                    percent: 100,
                    file_path: Some(output_path.to_string()),
                    message: warn_msg,
                },
            );
            return Ok(bytes);
        }

        let _ = fs::remove_file(output_path);
        let msg = if stderr_text.is_empty() {
            format!("Export failed with status {}", status)
        } else {
            stderr_text
        };
        emit_err(&msg);
        return Err(msg);
    }

    let _ = app.emit(
        "db-transfer-progress",
        DbTransferProgressEvent {
            op: "export".to_string(),
            target: target_label.to_string(),
            stage: "done".to_string(),
            bytes_processed: bytes,
            total_bytes: bytes,
            percent: 100,
            file_path: Some(output_path.to_string()),
            message: format!("Exported {} successfully.", target_label),
        },
    );

    Ok(bytes)
}

/// Sanitizes a single SQL line during import so MySQL 8, RDS, and MariaDB dumps
/// import cleanly into local MariaDB without collation, GTID, sandbox, or DEFINER errors.
fn sanitize_sql_import_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    // Skip MariaDB dump sandbox pragma (fails when piped to mismatched clients)
    if trimmed.starts_with("/*!999999\\-") || trimmed.starts_with("/*!999999-") {
        return None;
    }
    // Skip RDS / MySQL 8 GTID & binlog directives unsupported on local MariaDB
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with("SET @@GLOBAL.GTID_PURGED")
        || upper.starts_with("SET @@SESSION.SQL_LOG_BIN")
    {
        return None;
    }

    let mut out = if line.contains("DEFINER=") || line.contains("definer=") {
        strip_sql_definer_line(line)
    } else {
        line.to_string()
    };

    // Rewrite MySQL 8.0 0900 collations to MariaDB-compatible equivalents
    if out.contains("_0900_") {
        out = out
            .replace("utf8mb4_0900_ai_ci", "utf8mb4_unicode_ci")
            .replace("utf8mb4_0900_as_ci", "utf8mb4_unicode_ci")
            .replace("utf8mb4_0900_as_cs", "utf8mb4_unicode_ci")
            .replace("utf8mb4_0900_bin", "utf8mb4_bin")
            .replace("utf8mb3_0900_ai_ci", "utf8mb3_unicode_ci");
    }

    Some(out)
}

fn stream_sanitized_sql<R: Read, W: Write>(reader: R, writer: &mut W) -> std::io::Result<()> {
    writer.write_all(
        b"SET FOREIGN_KEY_CHECKS=0;\nSET UNIQUE_CHECKS=0;\nSET SQL_MODE='NO_AUTO_VALUE_ON_ZERO';\n",
    )?;
    let mut buf_reader = BufReader::with_capacity(256 * 1024, reader);
    let mut raw_line = Vec::with_capacity(8192);
    loop {
        raw_line.clear();
        let n = buf_reader.read_until(b'\n', &mut raw_line)?;
        if n == 0 {
            break;
        }
        // Only run string sanitization on valid UTF-8 lines that contain known MySQL/MariaDB hazards;
        // binary BLOB INSERT lines pass through verbatim without allocating or corrupting bytes.
        if let Ok(text) = std::str::from_utf8(&raw_line) {
            if text.contains("999999")
                || text.contains("GTID_PURGED")
                || text.contains("SQL_LOG_BIN")
                || text.contains("DEFINER=")
                || text.contains("definer=")
                || text.contains("_0900_")
            {
                if let Some(cleaned) = sanitize_sql_import_line(text) {
                    writer.write_all(cleaned.as_bytes())?;
                }
                continue;
            }
        }
        writer.write_all(&raw_line)?;
    }
    writer.write_all(b"\nSET FOREIGN_KEY_CHECKS=1;\nSET UNIQUE_CHECKS=1;\n")?;
    writer.flush()
}

fn run_import_from_file(
    app: &AppHandle,
    server: &DbServerProfile,
    target_label: &str,
    target_db: Option<&str>,
    input_path: &str,
) -> Result<(), String> {
    let emit_err = |msg: &str| {
        let _ = app.emit(
            "db-transfer-progress",
            DbTransferProgressEvent {
                op: "import".to_string(),
                target: target_label.to_string(),
                stage: "error".to_string(),
                bytes_processed: 0,
                total_bytes: 0,
                percent: 0,
                file_path: Some(input_path.to_string()),
                message: msg.to_string(),
            },
        );
    };

    let total_bytes = Path::new(input_path)
        .metadata()
        .map(|m| m.len())
        .unwrap_or(0);

    let _ = app.emit(
        "db-transfer-progress",
        DbTransferProgressEvent {
            op: "import".to_string(),
            target: target_label.to_string(),
            stage: "running".to_string(),
            bytes_processed: 0,
            total_bytes,
            percent: 0,
            file_path: Some(input_path.to_string()),
            message: format!("Importing into {}...", target_label),
        },
    );

    let bin = match find_mysql_client() {
        Ok(b) => b,
        Err(e) => {
            emit_err(&e);
            return Err(e);
        }
    };

    let mut in_file = match File::open(input_path) {
        Ok(f) => f,
        Err(e) => {
            let msg = format!("Failed opening SQL file {}: {}", input_path, e);
            emit_err(&msg);
            return Err(msg);
        }
    };
    let mut magic = [0u8; 2];
    let is_gzip = in_file.read_exact(&mut magic).is_ok() && magic == [0x1f, 0x8b];
    if let Err(e) = in_file.seek(SeekFrom::Start(0)) {
        let msg = format!("Failed rewinding SQL file {}: {}", input_path, e);
        emit_err(&msg);
        return Err(msg);
    }

    let _creds_file = if !server.password.is_empty() {
        match TempMysqlDefaultsFile::create(&server.password) {
            Ok(cf) => Some(cf),
            Err(e) => {
                emit_err(&e);
                return Err(e);
            }
        }
    } else {
        None
    };

    let mut cmd = Command::new(bin);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    if let Some(ref cf) = _creds_file {
        cmd.arg(cf.arg());
    }
    cmd.args(build_base_args(server))
        .arg("--default-character-set=utf8mb4")
        .arg("--max-allowed-packet=512M")
        .arg("--force");
    if let Some(db) = target_db {
        cmd.arg(db);
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("Failed running SQL import: {}", e);
            emit_err(&msg);
            return Err(msg);
        }
    };

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Failed opening SQL client stdin".to_string())?;
    let stderr = child.stderr.take();

    let stderr_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut se) = stderr {
            let _ = se.read_to_string(&mut buf);
        }
        buf
    });

    let progress_reader = ProgressReader {
        inner: BufReader::new(in_file),
        read_bytes: 0,
        last_emitted: 0,
        total_bytes,
        app: app.clone(),
        target: target_label.to_string(),
        file_path: input_path.to_string(),
    };

    let mut buf_stdin = BufWriter::with_capacity(256 * 1024, stdin);
    let copy_res = if is_gzip {
        let decoder = GzDecoder::new(progress_reader);
        stream_sanitized_sql(decoder, &mut buf_stdin)
    } else {
        stream_sanitized_sql(progress_reader, &mut buf_stdin)
    };
    drop(buf_stdin);

    let status = child
        .wait()
        .map_err(|e| format!("Failed waiting for SQL import: {}", e))?;
    let stderr_text = stderr_handle.join().unwrap_or_default().trim().to_string();

    let real_errors: Vec<&str> = stderr_text
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty()
                && !t.starts_with("Warning:")
                && !t.contains("Using a password on the command line")
        })
        .collect();

    if !status.success() {
        let is_fatal_conn = real_errors.iter().any(|l| {
            l.contains("ERROR 2002")
                || l.contains("ERROR 2003")
                || l.contains("ERROR 1045")
                || l.contains("ERROR 1049")
        });
        if is_fatal_conn || copy_res.is_err() {
            let msg = if real_errors.is_empty() {
                format!("Import failed with status {}", status)
            } else {
                real_errors.join("\n")
            };
            emit_err(&msg);
            return Err(msg);
        }

        let first_warn = real_errors
            .first()
            .copied()
            .unwrap_or("Some statements were skipped.");
        let _ = app.emit(
            "db-transfer-progress",
            DbTransferProgressEvent {
                op: "import".to_string(),
                target: target_label.to_string(),
                stage: "warning".to_string(),
                bytes_processed: total_bytes,
                total_bytes,
                percent: 100,
                file_path: Some(input_path.to_string()),
                message: format!(
                    "Imported into {} with {} warning(s): {}",
                    target_label,
                    real_errors.len(),
                    first_warn
                ),
            },
        );
        return Ok(());
    }
    if let Err(e) = copy_res {
        let msg = format!("Failed streaming SQL backup: {}", e);
        emit_err(&msg);
        return Err(msg);
    }

    let _ = app.emit(
        "db-transfer-progress",
        DbTransferProgressEvent {
            op: "import".to_string(),
            target: target_label.to_string(),
            stage: "done".to_string(),
            bytes_processed: total_bytes,
            total_bytes,
            percent: 100,
            file_path: Some(input_path.to_string()),
            message: format!("Imported into {} successfully.", target_label),
        },
    );

    Ok(())
}

pub fn export_database(
    app: &AppHandle,
    server_id: &str,
    database: &str,
    output_path: &str,
) -> Result<u64, String> {
    let server = get_server_by_id(server_id)?;
    let clean_db = validate_identifier(database, "Database name")?;
    run_dump_to_file(
        app,
        &server,
        &clean_db,
        std::slice::from_ref(&clean_db),
        output_path,
    )
}

pub fn import_database(
    app: &AppHandle,
    server_id: &str,
    database: &str,
    input_path: &str,
) -> Result<(), String> {
    let server = get_server_by_id(server_id)?;
    let clean_db = validate_identifier(database, "Database name")?;
    run_import_from_file(app, &server, &clean_db, Some(&clean_db), input_path)
}

pub fn export_server(
    app: &AppHandle,
    server_id: &str,
    output_path: &str,
) -> Result<(u64, usize), String> {
    let server = get_server_by_id(server_id)?;
    let schemas_raw = run_sql_query(
        &server,
        "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME;",
    )?;
    let mut user_dbs = Vec::new();
    for line in schemas_raw.lines() {
        let name = line.trim();
        if name.is_empty() || SYSTEM_SCHEMAS.contains(&name) {
            continue;
        }
        user_dbs.push(validate_identifier(name, "Database name")?);
    }
    if user_dbs.is_empty() {
        return Err("No user databases found on this server to export.".to_string());
    }

    let mut args = vec!["--databases".to_string()];
    args.extend(user_dbs.iter().cloned());
    let bytes = run_dump_to_file(app, &server, &server.name, &args, output_path)?;
    Ok((bytes, user_dbs.len()))
}

pub fn import_server(app: &AppHandle, server_id: &str, input_path: &str) -> Result<(), String> {
    let server = get_server_by_id(server_id)?;
    run_import_from_file(app, &server, &server.name, None, input_path)
}

pub fn list_database_tables(server_id: &str, database: &str) -> Result<Vec<DbTableInfo>, String> {
    let server = get_server_by_id(server_id)?;
    let clean_db = validate_identifier(database, "Database name")?;
    let sql = format!(
        "SELECT TABLE_NAME, COALESCE(TABLE_TYPE, 'BASE TABLE'), COALESCE(ENGINE, ''), COALESCE(TABLE_ROWS, 0), COALESCE(DATA_LENGTH + INDEX_LENGTH, 0), COALESCE(TABLE_COLLATION, '') FROM information_schema.TABLES WHERE TABLE_SCHEMA = '{}' ORDER BY TABLE_NAME;",
        escape_sql_str(&clean_db)
    );
    let raw = run_sql_query(&server, &sql)?;
    let tables = raw
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 6 {
                return None;
            }
            let name = parts[0].trim().to_string();
            if name.is_empty() {
                return None;
            }
            Some(DbTableInfo {
                name,
                table_type: parts[1].trim().to_string(),
                engine: parts[2].trim().to_string(),
                row_count: parts[3].trim().parse().unwrap_or(0),
                size_bytes: parts[4].trim().parse().unwrap_or(0),
                collation: parts[5].trim().to_string(),
            })
        })
        .collect();
    Ok(tables)
}

pub fn inspect_table(
    server_id: &str,
    database: &str,
    table: &str,
    limit: u32,
    offset: u64,
) -> Result<DbTablePreview, String> {
    let server = get_server_by_id(server_id)?;
    let clean_db = validate_identifier(database, "Database name")?;
    let clean_table = validate_identifier(table, "Table name")?;
    let capped_limit = limit.clamp(1, 200);

    let cols_sql = format!(
        "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COALESCE(COLUMN_KEY, ''), COALESCE(COLUMN_DEFAULT, '__WB_NULL__'), COALESCE(EXTRA, '') FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = '{}' AND TABLE_NAME = '{}' ORDER BY ORDINAL_POSITION;",
        escape_sql_str(&clean_db),
        escape_sql_str(&clean_table)
    );
    let cols_raw = run_sql_query(&server, &cols_sql)?;
    let columns: Vec<DbColumnInfo> = cols_raw
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 6 {
                return None;
            }
            let def = parts[4].trim();
            let default_value = if def == "__WB_NULL__" || def == "NULL" {
                None
            } else {
                Some(def.to_string())
            };
            Some(DbColumnInfo {
                name: parts[0].trim().to_string(),
                col_type: parts[1].trim().to_string(),
                nullable: parts[2].trim().eq_ignore_ascii_case("YES"),
                key: parts[3].trim().to_string(),
                default_value,
                extra: parts[5].trim().to_string(),
            })
        })
        .collect();

    let count_sql = format!("SELECT COUNT(*) FROM `{}`.`{}`;", clean_db, clean_table);
    let total_rows = run_sql_query(&server, &count_sql)
        .ok()
        .and_then(|s| s.lines().next().and_then(|l| l.trim().parse::<u64>().ok()))
        .unwrap_or(0);

    let rows_sql = format!(
        "SELECT * FROM `{}`.`{}` LIMIT {} OFFSET {};",
        clean_db, clean_table, capped_limit, offset
    );
    let rows_raw = run_sql_query(&server, &rows_sql).unwrap_or_default();
    let rows: Vec<Vec<String>> = rows_raw
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| line.split('\t').map(|cell| cell.to_string()).collect())
        .collect();

    Ok(DbTablePreview {
        table: clean_table,
        columns,
        rows,
        total_rows,
        limit: capped_limit,
        offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_sql_identifiers_and_blocks_injection() {
        assert_eq!(validate_identifier("laravel_app", "DB").unwrap(), "laravel_app");
        assert!(validate_identifier("bad; DROP TABLE x;--", "DB").is_err());
        assert!(validate_identifier("has space", "DB").is_err());
        assert!(validate_identifier("", "DB").is_err());
    }

    #[test]
    fn escapes_sql_strings_cleanly() {
        assert_eq!(escape_sql_str("p@ss'w\\ord"), "p@ss''w\\\\ord");
    }

    #[test]
    fn strips_sql_definer_clauses_cleanly() {
        let view_line = "CREATE ALGORITHM=UNDEFINED DEFINER=`cpanel_user`@`localhost` SQL SECURITY DEFINER VIEW `v_users` AS select 1;";
        let cleaned = strip_sql_definer_line(view_line);
        assert!(!cleaned.contains("DEFINER="));
        assert!(cleaned.contains("VIEW `v_users`"));

        let cond_line = "/*!50013 DEFINER=`root`@`localhost` SQL SECURITY DEFINER */\n";
        assert_eq!(strip_sql_definer_line(cond_line), "\n");
    }
}
