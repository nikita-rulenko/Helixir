use super::RequiredLookup;

pub(super) fn parse_required_lookup(
    query: &str,
    expression: &str,
) -> Result<RequiredLookup, String> {
    let (offset, marker) = typed_lookup_start(expression)
        .ok_or_else(|| format!("{query}: required lookup has no typed source: {expression}"))?;
    let label_start = offset + marker.len();
    let label_end = expression[label_start..]
        .find('>')
        .map(|relative| label_start + relative)
        .ok_or_else(|| format!("{query}: malformed node lookup: {expression}"))?;
    let collection = to_snake_case(&expression[label_start..label_end]);
    if is_direct_id_lookup(expression) {
        let root = typed_root(expression)
            .ok_or_else(|| format!("{query}: malformed direct-id lookup: {expression}"))?;
        let parameter = root
            .split_once(">(")
            .and_then(|(_, suffix)| suffix.strip_suffix(')'))
            .ok_or_else(|| format!("{query}: malformed direct-id lookup: {expression}"))?;
        return Ok(RequiredLookup {
            collection,
            property: "id".to_owned(),
            parameter: Some(parameter.trim().to_owned()),
            literal: None,
        });
    }
    if let Some((property, operand)) = typed_index_lookup(expression) {
        let literal = operand
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .map(str::to_owned);
        return Ok(RequiredLookup {
            collection,
            property: property.to_owned(),
            parameter: literal.is_none().then(|| operand.to_owned()),
            literal,
        });
    }
    let property_start = expression
        .find("_::{")
        .map(|position| position + 4)
        .ok_or_else(|| format!("{query}: FIRST lookup lacks property: {expression}"))?;
    let property_end = expression[property_start..]
        .find('}')
        .map(|relative| property_start + relative)
        .ok_or_else(|| format!("{query}: malformed FIRST property: {expression}"))?;
    let eq_start = expression[property_end..]
        .find("::EQ(")
        .map(|relative| property_end + relative + 5)
        .ok_or_else(|| format!("{query}: FIRST lookup lacks EQ: {expression}"))?;
    let eq_end = expression[eq_start..]
        .find(')')
        .map(|relative| eq_start + relative)
        .ok_or_else(|| format!("{query}: malformed FIRST EQ: {expression}"))?;
    let operand = expression[eq_start..eq_end].trim();
    let literal = operand
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(str::to_owned);
    Ok(RequiredLookup {
        collection,
        property: expression[property_start..property_end].to_owned(),
        parameter: literal.is_none().then(|| operand.to_owned()),
        literal,
    })
}

pub(super) fn is_direct_id_lookup(expression: &str) -> bool {
    typed_root(expression)
        .and_then(|root| root.split_once(">("))
        .and_then(|(_, arguments)| arguments.strip_suffix(')'))
        .is_some_and(|arguments| {
            let arguments = arguments.trim();
            !arguments.is_empty() && !arguments.starts_with('{')
        })
}

fn typed_index_lookup(expression: &str) -> Option<(&str, &str)> {
    let root = typed_root(expression)?;
    let (_, arguments) = root.split_once(">(")?;
    let body = arguments
        .strip_suffix(')')?
        .trim()
        .strip_prefix('{')?
        .strip_suffix('}')?;
    let (property, operand) = body.split_once(':')?;
    let property = property.trim();
    let operand = operand.trim();
    (!property.is_empty() && !operand.is_empty()).then_some((property, operand))
}

fn typed_root(expression: &str) -> Option<&str> {
    let (offset, _) = typed_lookup_start(expression)?;
    Some(
        expression[offset..]
            .split("::")
            .next()
            .unwrap_or(expression),
    )
}

fn typed_lookup_start(expression: &str) -> Option<(usize, &'static str)> {
    ["N<", "E<", "V<"]
        .into_iter()
        .flat_map(|marker| {
            expression
                .match_indices(marker)
                .map(move |(offset, _)| (offset, marker))
        })
        .filter(|(offset, _)| {
            *offset == 0
                || (!expression.as_bytes()[offset - 1].is_ascii_alphanumeric()
                    && expression.as_bytes()[offset - 1] != b'_')
        })
        .min_by_key(|(offset, _)| *offset)
}

fn to_snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_uppercase() && index > 0 {
            output.push('_');
        }
        output.extend(character.to_lowercase());
    }
    output
}
