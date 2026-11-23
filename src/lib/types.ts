export type DeleteMode = 'trash' | 'permanent';

export type ProjectKind = 'laravel' | 'php' | 'sveltekit' | 'node' | 'flutter' | 'rust';

export interface LocalSite {
	domain: string;
	project_path: string;
	server: 'caddy' | 'apache' | string;
	mode: 'php' | 'proxy' | 'static' | string;
	proxy_port?: number | null;
	docroot_subpath?: string | null;
	https?: boolean;
	expose_lan?: boolean;
}

export interface NetworkStatus {
	lan_ip: string | null;
	hosts_path: string;
	synced_domains: string[];
	missing_domains: string[];
	caddy_available: boolean;
	caddy_port: number;
	caddy_https_port: number;
	apache_port: number;
	portless_enabled: boolean;
}

export interface DbServerProfile {
	id: string;
	name: string;
	engine?: 'mariadb' | 'mysql' | 'postgresql' | 'redis' | string;
	host: string;
	port: number;
	username: string;
	password: string;
}

export interface WorkbenchConfig {
	scan_roots: string[];
	delete_mode: DeleteMode;
	stale_days: number;
	theme: 'system' | 'light' | 'dark' | string;
	defaults?: Record<string, string>;
	service_ports?: Record<string, number>;
	sites?: LocalSite[];
	db_servers?: DbServerProfile[];
}

export interface ArtifactInfo {
	rel_path: string;
	abs_path: string;
	label: string;
	size_bytes: number;
	clean_contents_only: boolean;
}

export interface ProjectInfo {
	name: string;
	path: string;
	kind: ProjectKind;
	secondary_kinds: ProjectKind[];
	artifacts: ArtifactInfo[];
	total_reclaimable_bytes: number;
	last_modified_epoch: number;
	idle_days: number;
	hibernated: boolean;
	version_hints: Record<string, string>;
	wake_commands: string[];
}

export interface ScanResult {
	projects: ProjectInfo[];
	total_reclaimable_bytes: number;
	scan_duration_ms: number;
}

export interface HibernateResult {
	project_path: string;
	project_name: string;
	bytes_reclaimed: number;
	deleted_artifacts: string[];
	errors: string[];
}

export interface GlobalCacheInfo {
	id: string;
	name: string;
	ecosystem: string;
	path: string;
	size_bytes: number;
	exists: boolean;
	description: string;
}

export interface CleanupRecord {
	id: number;
	target_path: string;
	target_name: string;
	category: string;
	bytes_reclaimed: number;
	delete_mode: string;
	created_at: string;
}

export interface CleanupStats {
	total_bytes_reclaimed: number;
	total_cleanups: number;
	recent: CleanupRecord[];
}

export interface ShellIntegrationStatus {
	shell_name: string;
	profile_path: string;
	shims_path: string;
	installed: boolean;
	workbench_home: string;
}

export interface UninstallReport {
	shell_profiles_cleaned: string[];
	workbench_home_removed: boolean;
	removed_path: string;
	user_projects_touched: boolean;
}

export interface DetectedTool {
	id: string;
	name: string;
	category: 'runtime' | 'service' | string;
	installed: boolean;
	version: string | null;
	binary_path: string | null;
	source: string | null;
}

export interface WakeLogEvent {
	project_path: string;
	line: string;
	stream: 'stdout' | 'stderr' | 'info' | string;
}

export interface WakeDoneEvent {
	project_path: string;
	success: boolean;
	message: string;
}

export interface RuntimeVersionEntry {
	tool: string;
	version: string;
	label: string;
	channel: string;
	size_hint_mb: number;
	installed: boolean;
	is_default: boolean;
	is_external: boolean;
	source: string | null;
	disk_bytes: number;
	available_on_platform: boolean;
	install_path: string | null;
}

export interface RuntimeProgressEvent {
	tool: string;
	version: string;
	stage: 'downloading' | 'extracting' | 'done' | string;
	percent: number;
	downloaded_bytes: number;
	total_bytes: number;
}

export interface RustupToolchain {
	name: string;
	is_default: boolean;
}

export interface RustupStatus {
	available: boolean;
	toolchains: RustupToolchain[];
}

export interface ShimSyncReport {
	shims_dir: string;
	mode: string;
	shims_created: string[];
}

export interface ServiceStatusInfo {
	id: string;
	name: string;
	version: string;
	description: string;
	port: number;
	status: 'running' | 'stopped' | string;
	pid: number | null;
	installed_managed: boolean;
	host_binary: string | null;
	host_source: string | null;
	downloadable: boolean;
	size_hint_mb: number;
	disk_bytes: number;
	url: string | null;
	logs: string[];
}

export interface ServiceLogEvent {
	service_id: string;
	line: string;
}

export interface ServiceLogsResponse {
	service_id: string;
	source_path: string | null;
	lines: string[];
}

export interface ServiceProgressEvent {
	service_id: string;
	stage: 'downloading' | 'extracting' | 'done' | string;
	percent: number;
}

export interface PhpExtensionInfo {
	name: string;
	loaded: boolean;
	built_in: boolean;
	available_dynamic: boolean;
	enabled_in_ini: boolean;
	is_laravel_core: boolean;
	is_zend: boolean;
}

export interface PhpIniSettings {
	memory_limit: string;
	upload_max_filesize: string;
	post_max_size: string;
	max_execution_time: string;
	opcache_enable: boolean;
}

export interface PhpVersionDetails {
	version: string;
	ini_path: string;
	extensions: PhpExtensionInfo[];
	settings: PhpIniSettings;
}

export interface DatabaseInfo {
	name: string;
	table_count: number;
	size_bytes: number;
	charset: string;
	collation: string;
	is_system: boolean;
}

export interface DbUserGrant {
	database: string;
	privilege: string;
}

export interface DbUserInfo {
	username: string;
	host: string;
	is_protected: boolean;
	grants: DbUserGrant[];
}

export interface DbServerInspection {
	server: DbServerProfile;
	connected: boolean;
	server_version: string | null;
	error: string | null;
	databases: DatabaseInfo[];
	users: DbUserInfo[];
}

export interface DbTableInfo {
	name: string;
	table_type: string;
	engine: string;
	row_count: number;
	size_bytes: number;
	collation: string;
}

export interface DbColumnInfo {
	name: string;
	col_type: string;
	nullable: boolean;
	key: string;
	default_value: string | null;
	extra: string;
}

export interface DbTablePreview {
	table: string;
	columns: DbColumnInfo[];
	rows: string[][];
	total_rows: number;
	limit: number;
	offset: number;
}

export interface UpdateCheckResult {
	current_version: string;
	latest_version: string | null;
	update_available: boolean;
	release_url: string | null;
	checked: boolean;
	error: string | null;
}

export interface DbTransferProgressEvent {
	op: 'export' | 'import' | string;
	target: string;
	stage: 'running' | 'done' | 'warning' | 'error' | string;
	bytes_processed: number;
	total_bytes: number;
	percent: number;
	file_path: string | null;
	message: string;
}

export interface SystemStats {
	cpu_percent: number;
	cpu_cores: number;
	cpu_brand: string;
	load_avg_one: number;
	load_avg_five: number;
	load_avg_fifteen: number;
	memory_used_bytes: number;
	memory_total_bytes: number;
	memory_percent: number;
	swap_used_bytes: number;
	swap_total_bytes: number;
	net_rx_bytes_per_sec: number;
	net_tx_bytes_per_sec: number;
	uptime_secs: number;
	os_name: string;
}
