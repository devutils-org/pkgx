use serde_json::Value;
fn id(value: &Value) -> String {
    let provider = value["provider"].as_str().unwrap_or("unknown");
    let name = value["name"].as_str().unwrap_or("unknown");
    match value["architecture_or_scope"].as_str() {
        Some(scope) => format!("{provider}:{name}:{scope}"),
        None => format!("{provider}:{name}"),
    }
}
fn string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
fn lines(values: &Value, prefix: &str) -> String {
    values
        .as_array()
        .map(|values| {
            values
                .iter()
                .map(|value| format!("{prefix}{}\n", string(value)))
                .collect()
        })
        .unwrap_or_default()
}
fn render(value: &Value) -> String {
    if let Some(items) = value.as_array() {
        return items.iter().map(render).collect::<Vec<_>>().join("\n");
    }
    if value.get("started_unix_ms").is_some() && value.get("result").is_some() {
        let result = &value["result"];
        let state = if result.is_null() {
            "running/interrupted/unknown"
        } else if result["success"] == true {
            "success"
        } else {
            "failed"
        };
        let mut text = format!(
            "Transaction {} — {state} — started Unix ms {}\n",
            string(&value["id"]),
            value["started_unix_ms"]
        );
        text.push_str(&render(&value["plan"]));
        if !result.is_null() {
            text.push_str(&render(result));
        }
        return text;
    }
    if value.get("operations").is_some() {
        let mut text = String::new();
        for op in value["operations"].as_array().into_iter().flatten() {
            text.push_str(&format!(
                "{} {} [{}]\n  {}\n",
                string(&op["kind"]),
                id(&op["package"]),
                string(&op["risk"]),
                string(&op["native_detail"])
            ));
        }
        if text.is_empty() {
            text.push_str("No package operations.\n");
        }
        if value["uncertain"] == true {
            text.push_str("Native preview is uncertain; review the limitations below.\n");
        }
        text.push_str(&lines(&value["warnings"], "Warning: "));
        return text;
    }
    if let Some(providers) = value
        .get("providers")
        .and_then(Value::as_array)
        .filter(|_| value.get("diff").is_some())
    {
        let mut text = render(&value["diff"]);
        for provider in providers {
            text.push_str(&format!(
                "\nProvider {}:\n{}",
                string(&provider["provider"]),
                render(&provider["plan"])
            ));
        }
        return text;
    }
    if value.get("missing").is_some() {
        let mut text = String::new();
        for (key, label) in [
            ("missing", "missing"),
            ("present", "present"),
            ("undeclared", "undeclared; retained"),
            ("removals", "managed removal"),
        ] {
            for package in value[key].as_array().into_iter().flatten() {
                text.push_str(&format!("{label}\t{}\n", id(package)));
            }
        }
        text.push_str(&lines(&value["warnings"], "Warning: "));
        return text;
    }
    if value.get("updated_unix_secs").is_some() && value.get("uninstall").is_some() {
        // Script receipts and their action records: never transaction wording.
        let mut text = format!(
            "Recipe {} {}: {}\n",
            value["name"].as_str().unwrap_or("?"),
            value["pinned_version"].as_str().unwrap_or(""),
            value["status"].as_str().unwrap_or("unknown"),
        );
        if let Some(resources) = value["resources"].as_array() {
            for resource in resources {
                text.push_str(&format!(
                    "  {} {} ({})\n",
                    resource["kind"].as_str().unwrap_or("?"),
                    resource["path"].as_str().unwrap_or("?"),
                    resource["evidence"]["type"].as_str().unwrap_or("?"),
                ));
            }
        }
        return text;
    }
    if value.get("retained").is_some() {
        return format!(
            "Script success: {}\nScript status: {}\n{}{}{}{}",
            value["success"],
            value["native_status"],
            lines(&value["errors"], "Error: "),
            lines(&value["retained"], "Retained for manual review: "),
            lines(&value["observed"], "Observed (undeclared, not owned): "),
            lines(&value["warnings"], "Warning: "),
        );
    }
    if value.get("native_status").is_some() {
        return format!(
            "Transaction success: {}\nNative status: {}\nVerified: {}\n{}Rollback attempted: {}\n",
            value["success"],
            value["native_status"],
            value["verified"],
            lines(&value["errors"], "Error: "),
            value["rollback_attempted"]
        );
    }
    if value.get("id").is_some() && value.get("summary").is_some() {
        return format!("{}\t{}\n", id(&value["id"]), string(&value["summary"]));
    }
    if let Some(records) = value.get("records").and_then(Value::as_array) {
        let mut text = format!("{}\n", id(&value["id"]));
        for record in records {
            if let Some(fields) = record.as_object() {
                for (key, value) in fields {
                    text.push_str(&format!("{key}: {}\n", string(value)));
                }
                text.push('\n');
            }
        }
        return text;
    }
    if value.get("expressions").is_some() && value.get("resolved_graph").is_some() {
        let mut text = format!(
            "{}\t{}\nSource: {}\nNative dependency expressions (not a resolved graph):\n",
            id(&value["package"]),
            string(&value["installed_version"]),
            string(&value["source"])
        );
        for expression in value["expressions"].as_array().into_iter().flatten() {
            text.push_str(&format!(
                "{}: {}\n",
                string(&expression["relation"]),
                string(&expression["expression"])
            ));
        }
        if value["expressions"]
            .as_array()
            .is_some_and(|v| v.is_empty())
        {
            text.push_str("No expressions reported by this query.\n");
        }
        text.push_str(&lines(&value["limitations"], "Limitation: "));
        return text;
    }
    if value.get("installed_reverse_dependencies").is_some() {
        let p = &value["package"];
        return format!(
            "{}\t{}\nReason: {}\nOrigin: {}\nConfidence: {}\nNative reverse dependencies:\n{}\n{}",
            id(&p["id"]),
            string(&p["version"]),
            string(&p["install_reason"]),
            string(&value["origin"]),
            string(&value["confidence"]),
            string(&value["installed_reverse_dependencies"]),
            lines(&value["limitations"], "Limitation: ")
        );
    }
    // Unrecognized diagnostic types retain all data with JSON escaping. Avoid
    // swallowing future fields by inventing a lossy generic table.
    serde_json::to_string_pretty(value).expect("JSON value serialization")
}
pub fn human(value: &impl serde::Serialize) -> Result<String, serde_json::Error> {
    let text = render(&serde_json::to_value(value)?);
    // Native metadata must not inject terminal control sequences.
    Ok(text
        .chars()
        .flat_map(|ch| {
            if ch.is_control() && ch != '\n' && ch != '\t' {
                ch.escape_default().collect::<Vec<_>>()
            } else {
                vec![ch]
            }
        })
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_preserves_operations_and_hides_internal_inventory_token() {
        let value = serde_json::json!({"observed_state_fingerprint":"internal inventory", "uncertain":true,"warnings":["review"],"operations":[{"kind":"install","package":{"provider":"apt","name":"curl","architecture_or_scope":"amd64"},"risk":"requires_review","native_detail":"Inst curl (8.0 [amd64])"}]});
        let text = human(&value).unwrap();
        assert!(text.contains("install apt:curl:amd64"));
        assert!(text.contains("Inst curl"));
        assert!(text.contains("review"));
        assert!(!text.contains("internal inventory"));
    }
    #[test]
    fn native_descriptions_cannot_inject_terminal_sequences() {
        let value =
            serde_json::json!({"id":{"provider":"apt","name":"curl"},"summary":"bad\u{1b}[2J"});
        assert!(!human(&value).unwrap().contains('\u{1b}'));
    }
}
