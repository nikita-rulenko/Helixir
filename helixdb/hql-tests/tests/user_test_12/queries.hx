QUERY CreateIndicator(
    name: String,
    description: String,
    source: String,
    username: String,
    run_name: String,
    run_start: Date,
    run_end: Date,
    tz: String,
    indicator_class: String,
    config_origin: String,
    tags: String,
    asset_class: String,
    family: String,
    measure_type: String,
    currency_code: String,
    index_name: String,
    forward_tenor: String,
    tenor: String
) =>
    indicator <- AddN<Indicator>({
        name: name,
        description: description,
        source: source,
        username: username,
        run_name: run_name,
        run_start: run_start,
        run_end: run_end,
        tz: tz,
        indicator_class: indicator_class,
        config_origin: config_origin,
        tags: tags,
        asset_class: asset_class,
        family: family,
        measure_type: measure_type,
        currency_code: currency_code,
        index_name: index_name,
        forward_tenor: forward_tenor,
        tenor: tenor
    })
    RETURN indicator

QUERY CreateTimeParameter(
    name: String,
    value: String,
    classification: String
) =>
    time_parameter <- AddN<TimeParameter>({
        name: name,
        value: value,
        classification: classification
    })
    RETURN time_parameter


QUERY LinkIndicatorToTimeParameter(
    indicator_id: ID,
    time_parameter_id: ID
) =>
    link <- AddE<HasTimeParameter>::From(indicator_id)::To(time_parameter_id)
    RETURN link

QUERY GetIndicatorsWithTimeParams(time_vals: [String]) =>
    // Find indicators connected to ALL matching time parameters (set intersection)
    indicators <- N<TimeParameter>::WHERE(_::{value}::IS_IN(time_vals))::INTERSECT(_::In<HasTimeParameter>)
    RETURN indicators
