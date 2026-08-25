N::TimeParameter {
    UNIQUE INDEX name: String,
    value: String,
    classification: String DEFAULT "unknown"
}

N::Indicator {
    UNIQUE INDEX name: String,
    description: String DEFAULT "",
    INDEX source: String DEFAULT "",
    username: String DEFAULT "",
    run_name: String DEFAULT "",
    run_start: Date,
    run_end: Date,
    tz: String DEFAULT "",
    indicator_class: String DEFAULT "",
    config_origin: String DEFAULT "",
    tags: String DEFAULT "",
    asset_class: String DEFAULT "",
    INDEX family: String DEFAULT "",
    measure_type: String DEFAULT "",
    currency_code: String DEFAULT "",
    index_name: String DEFAULT "",
    forward_tenor: String DEFAULT "",
    tenor: String DEFAULT ""
}

E::HasTimeParameter {
    From: Indicator,
    To: TimeParameter
}