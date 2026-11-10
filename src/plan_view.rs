//! A plan as a dialog draws it: the lines, what was left out and why, the heads-ups. Built from
//! a local plan, or from the plan JSON the CLI prints (a plan made on another machine).

use serde_json::Value;

use crate::agents::Paths;
use crate::mcp::agents::MCP_AGENTS;
use crate::mcp::plan::McpPlan;
use crate::sync;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanView {
    pub lines: Vec<String>,
    /// What was left out, as (who, why): a skill for a sync, an agent for an MCP plan.
    pub skipped: Vec<(String, String)>,
    pub warnings: Vec<String>,
    /// The plan id the CLI prints, where the plan came from there.
    pub id: Option<String>,
}

impl From<(&sync::Plan, &Paths)> for PlanView {
    fn from((plan, _): (&sync::Plan, &Paths)) -> Self {
        PlanView {
            lines: plan.actions.iter().map(sync::Action::describe).collect(),
            skipped: plan.conflicts.iter().map(|c| (c.skill.clone(), c.reason.clone())).collect(),
            warnings: Vec::new(),
            id: None,
        }
    }
}

impl From<(&McpPlan, &Paths)> for PlanView {
    fn from((plan, paths): (&McpPlan, &Paths)) -> Self {
        PlanView {
            lines: plan.lines(paths),
            skipped: plan.skipped.iter().map(|(a, why)| (MCP_AGENTS[*a].name.to_string(), why.clone())).collect(),
            warnings: plan.warnings.clone(),
            id: Some(plan.id()),
        }
    }
}

impl PlanView {
    /// The plan JSON of the skills and mcp commands: `actions`, `conflicts` (skills) or
    /// `skipped` (mcp), `warnings`, `plan_id`.
    pub fn from_json(v: &Value) -> Result<PlanView, String> {
        let strings = |key: &str| -> Result<Vec<String>, String> {
            match v.get(key) {
                None | Some(Value::Null) => Ok(Vec::new()),
                Some(Value::Array(items)) => items.iter().map(|i| i.as_str().map(str::to_string).ok_or_else(|| format!("the plan's {key} has a non-text entry"))).collect(),
                Some(_) => Err(format!("the plan's {key} is not a list")),
            }
        };
        let pairs = |key: &str, who: &str| -> Result<Vec<(String, String)>, String> {
            let field = |item: &Value, f: &str| item.get(f).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("an entry of the plan's {key} has no {f}"));
            match v.get(key) {
                None | Some(Value::Null) => Ok(Vec::new()),
                Some(Value::Array(items)) => items.iter().map(|i| Ok((field(i, who)?, field(i, "reason")?))).collect(),
                Some(_) => Err(format!("the plan's {key} is not a list")),
            }
        };
        if !v.is_object() || v.get("actions").is_none() {
            return Err("not a plan: it has no actions".into());
        }
        let mut skipped = pairs("conflicts", "skill")?;
        skipped.extend(pairs("skipped", "agent")?.into_iter().map(|(agent, why)| {
            let name = MCP_AGENTS.iter().find(|a| a.id == agent).map_or(agent.clone(), |a| a.name.to_string());
            (name, why)
        }));
        Ok(PlanView { lines: strings("actions")?, skipped, warnings: strings("warnings")?, id: v.get("plan_id").and_then(Value::as_str).map(str::to_string) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_view_from_cli_json() {
        let skills = json!({
            "applied": false, "plan_id": "abc123",
            "actions": ["Link pdf into Claude Code"],
            "conflicts": [{ "skill": "docx", "reason": "Codex has its own copy" }],
        });
        let v = PlanView::from_json(&skills).unwrap();
        assert_eq!(v.lines, ["Link pdf into Claude Code"]);
        assert_eq!(v.skipped, [("docx".to_string(), "Codex has its own copy".to_string())]);
        assert!(v.warnings.is_empty());
        assert_eq!(v.id.as_deref(), Some("abc123"));

        let mcp = json!({
            "applied": false, "plan_id": "def456",
            "actions": ["Add plata to Claude Code"],
            "skipped": [{ "agent": MCP_AGENTS[0].id, "reason": "has it already" }],
            "warnings": ["Gemini CLI keeps headers in plain text"],
        });
        let v = PlanView::from_json(&mcp).unwrap();
        assert_eq!(v.lines, ["Add plata to Claude Code"]);
        assert_eq!(v.skipped, [(MCP_AGENTS[0].name.to_string(), "has it already".to_string())]);
        assert_eq!(v.warnings, ["Gemini CLI keeps headers in plain text"]);
        assert_eq!(v.id.as_deref(), Some("def456"));
    }

    #[test]
    fn plan_view_from_json_refuses_what_is_not_a_plan() {
        assert!(PlanView::from_json(&json!({ "error": "boom" })).is_err());
        assert!(PlanView::from_json(&json!({ "actions": "x" })).is_err());
    }
}
