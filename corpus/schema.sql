PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS runs (
    id INTEGER PRIMARY KEY,
    imported_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    metadata TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS cases (
    id TEXT PRIMARY KEY,
    source BLOB NOT NULL,
    family TEXT NOT NULL,
    depth INTEGER,
    seed TEXT,
    first_seen_run INTEGER,
    FOREIGN KEY (first_seen_run) REFERENCES runs(id)
);

CREATE TABLE IF NOT EXISTS observations (
    run_id INTEGER NOT NULL,
    case_id TEXT NOT NULL,
    implementation TEXT NOT NULL,
    status TEXT NOT NULL,
    seconds REAL,
    returncode INTEGER,
    stderr TEXT,
    PRIMARY KEY (run_id, case_id, implementation),
    FOREIGN KEY (run_id) REFERENCES runs(id),
    FOREIGN KEY (case_id) REFERENCES cases(id)
);

CREATE TABLE IF NOT EXISTS case_labels (
    case_id TEXT NOT NULL,
    label TEXT NOT NULL,
    first_seen_run INTEGER,
    PRIMARY KEY (case_id, label),
    FOREIGN KEY (case_id) REFERENCES cases(id),
    FOREIGN KEY (first_seen_run) REFERENCES runs(id)
);

CREATE INDEX IF NOT EXISTS observations_by_case ON observations(case_id, implementation);

CREATE TABLE IF NOT EXISTS observation_context (
    run_id INTEGER NOT NULL,
    case_id TEXT NOT NULL,
    implementation TEXT NOT NULL,
    phase TEXT NOT NULL,
    implementation_version TEXT,
    PRIMARY KEY (run_id, case_id, implementation),
    FOREIGN KEY (run_id, case_id, implementation)
        REFERENCES observations(run_id, case_id, implementation)
);

CREATE TABLE IF NOT EXISTS case_generators (
    case_id TEXT PRIMARY KEY,
    generator TEXT NOT NULL,
    parameters TEXT NOT NULL,
    FOREIGN KEY (case_id) REFERENCES cases(id)
);

CREATE TABLE IF NOT EXISTS resource_observations (
    run_id INTEGER NOT NULL,
    case_id TEXT NOT NULL,
    implementation TEXT NOT NULL,
    phase TEXT NOT NULL,
    outcome TEXT NOT NULL,
    status TEXT NOT NULL,
    sample_count INTEGER NOT NULL,
    median_seconds REAL,
    min_seconds REAL,
    max_seconds REAL,
    peak_rss_bytes INTEGER,
    baseline_rss_bytes INTEGER,
    time_limit_seconds REAL,
    memory_limit_bytes INTEGER,
    samples TEXT NOT NULL,
    details TEXT,
    PRIMARY KEY (run_id, case_id, implementation, phase),
    FOREIGN KEY (run_id) REFERENCES runs(id),
    FOREIGN KEY (case_id) REFERENCES cases(id)
);

CREATE INDEX IF NOT EXISTS resource_observations_by_case
    ON resource_observations(case_id, implementation, phase);

CREATE TABLE IF NOT EXISTS calibration_observations (
    run_id INTEGER NOT NULL,
    corpus TEXT NOT NULL,
    path TEXT NOT NULL,
    source_sha256 TEXT NOT NULL,
    source_bytes INTEGER NOT NULL,
    token_count INTEGER NOT NULL,
    ast_nodes INTEGER NOT NULL,
    statement_count INTEGER NOT NULL,
    identifier_count INTEGER NOT NULL,
    identifier_occurrences INTEGER NOT NULL,
    max_call_arguments INTEGER NOT NULL,
    max_parameters INTEGER NOT NULL,
    max_display_items INTEGER NOT NULL,
    max_slice_items INTEGER NOT NULL,
    max_interpolation_fields INTEGER NOT NULL,
    spam_status TEXT NOT NULL,
    parse_samples TEXT NOT NULL,
    parse_median_seconds REAL NOT NULL,
    compile_samples TEXT NOT NULL,
    compile_median_seconds REAL NOT NULL,
    ruff_status TEXT,
    ruff_seconds REAL,
    PRIMARY KEY (run_id, corpus, path),
    FOREIGN KEY (run_id) REFERENCES runs(id)
);

CREATE INDEX IF NOT EXISTS calibration_observations_by_run
    ON calibration_observations(run_id, corpus);
