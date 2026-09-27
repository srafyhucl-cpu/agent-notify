//! 任务信封模板解析测试（P1-5）：用户模板优先、默认兜底、告警不静默。
//!
//! 覆盖：用户配置模板生效 / 步骤自带模板优先 / 缺失回默认 / 文件缺失正常 /
//! 整份损坏回默认+告警 / 单条损坏只丢该条 / 序号非法与重复 / 空白模板未配置 /
//! 未知占位符告警+原样保留 / 与 P0 `render_envelope` 兼容。

use agentnotify_orchestration::{
    TemplateResolver, TemplateSource, TemplateWarningKind, Workflow, render_envelope,
};
use tempfile::tempdir;

/// 预置工作流（步骤 1/2/3 均无自带模板，走「用户配置 > 默认」解析）。
fn preset() -> Workflow {
    Workflow::preset(false).expect("预置工作流必须有效")
}

const PRESET_ID: &str = "preset-requirement-to-report";

/// 用户配置模板（按 workflow_id + step order 命中）生效，来源上报为 UserConfig。
#[test]
fn user_config_template_is_used_and_source_reported() {
    let json = r#"{"workflows":{"preset-requirement-to-report":{"steps":[
        {"order":1,"harness_template":"【{goal}】判断：{role}（Agent {agent_hint}）"},
        {"order":2,"harness_template":"【{goal}】规划：{role}"}
    ]}}}"#;
    let result = TemplateResolver::from_config_str(json);
    assert!(
        result.warnings.is_empty(),
        "加载不应有告警：{:?}",
        result.warnings
    );

    let wf = preset();
    let step1 = wf.step(1).expect("步骤 1 必须存在");
    let rendered = result
        .resolver
        .render_envelope(&wf, step1, "贪吃蛇", Some("planner"));
    assert_eq!(
        rendered.text, "【贪吃蛇】判断：orchestrator（Agent codex）",
        "用户模板必须替换占位符并生效：{}",
        rendered.text
    );
    assert!(rendered.warnings.is_empty(), "{:?}", rendered.warnings);

    let resolved = result
        .resolver
        .resolve(step1.harness_template.as_deref(), &wf.id, step1.order);
    assert_eq!(resolved.source, TemplateSource::UserConfig);
    assert_eq!(resolved.warning, None);

    // 步骤 2 也命中用户配置；步骤 3 未配置 → 内置默认
    let step2 = wf.step(2).expect("步骤 2 必须存在");
    let resolved2 = result
        .resolver
        .resolve(step2.harness_template.as_deref(), &wf.id, step2.order);
    assert_eq!(resolved2.source, TemplateSource::UserConfig);
    let step3 = wf.step(3).expect("步骤 3 必须存在");
    let resolved3 = result
        .resolver
        .resolve(step3.harness_template.as_deref(), &wf.id, step3.order);
    assert_eq!(resolved3.source, TemplateSource::BuiltinDefault);
}

/// 步骤自带模板（P0 行为）优先于用户配置文件模板。
#[test]
fn step_template_precedes_user_config() {
    let json = r#"{"workflows":{"preset-requirement-to-report":{"steps":[
        {"order":1,"harness_template":"配置文件模板 【{goal}】"}
    ]}}}"#;
    let result = TemplateResolver::from_config_str(json);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);

    let wf = preset();
    let mut step1 = wf.step(1).expect("步骤 1 必须存在").clone();
    step1.harness_template = Some("步骤自带模板 【{goal}】".to_string());

    let rendered = result
        .resolver
        .render_envelope(&wf, &step1, "贪吃蛇", Some("planner"));
    assert_eq!(rendered.text, "步骤自带模板 【贪吃蛇】");
    let resolved = result
        .resolver
        .resolve(step1.harness_template.as_deref(), &wf.id, step1.order);
    assert_eq!(resolved.source, TemplateSource::StepOverride);
}

/// 配置里没有该工作流 → 回退内置默认模板（来源 BuiltinDefault，无告警）。
#[test]
fn missing_user_config_falls_back_to_default() {
    let json = r#"{"workflows":{"another-workflow":{"steps":[
        {"order":1,"harness_template":"别的工作流模板"}
    ]}}}"#;
    let result = TemplateResolver::from_config_str(json);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);

    let wf = preset();
    let step1 = wf.step(1).expect("步骤 1 必须存在");
    let resolved = result
        .resolver
        .resolve(step1.harness_template.as_deref(), &wf.id, step1.order);
    assert_eq!(resolved.source, TemplateSource::BuiltinDefault);
    let rendered = result
        .resolver
        .render_envelope(&wf, step1, "贪吃蛇", Some("planner"));
    assert!(
        rendered.text.contains("【任务：贪吃蛇】"),
        "默认模板必须生效：{}",
        rendered.text
    );
    assert!(rendered.warnings.is_empty(), "{:?}", rendered.warnings);
}

/// 配置文件不存在 = 正常未配置：无告警，解析器只含内置默认。
#[test]
fn missing_config_file_is_normal_without_warnings() {
    let dir = tempdir().expect("临时目录必须可创建");
    let missing = dir.path().join("harness-templates.json");
    let result = TemplateResolver::from_config_file(&missing);
    assert!(
        result.warnings.is_empty(),
        "缺失文件不应告警：{:?}",
        result.warnings
    );
    assert_eq!(result.resolver.user_templates(), &Default::default());
}

/// 整份 JSON 损坏 → 一条 ConfigInvalid 告警（写清失败原因）+ 全部回退默认。
#[test]
fn corrupted_config_warns_and_falls_back_to_default() {
    let result = TemplateResolver::from_config_str("{ not valid json");
    assert_eq!(result.warnings.len(), 1);
    let warning = &result.warnings[0];
    assert_eq!(warning.kind, TemplateWarningKind::ConfigInvalid);
    assert!(warning.message.contains("解析失败"), "{}", warning.message);
    assert!(
        warning.message.contains("回退内置默认模板"),
        "必须写清回退原因：{}",
        warning.message
    );
    assert_eq!(result.resolver.user_templates(), &Default::default());

    // 渲染仍走默认，编排不瘫痪
    let wf = preset();
    let rendered =
        result
            .resolver
            .render_envelope(&wf, wf.step(1).unwrap(), "贪吃蛇", Some("planner"));
    assert!(
        rendered.text.contains("【任务：贪吃蛇】"),
        "{}",
        rendered.text
    );
}

/// 单个工作流条目损坏（类型错）→ 只丢该条 + 告警，其余工作流模板仍生效。
#[test]
fn entry_type_error_skips_only_that_workflow() {
    let json = r#"{"workflows":{
        "preset-requirement-to-report":{"steps":[{"order":1,"harness_template":"有效模板 【{goal}】"}]},
        "broken-wf":{"steps":[{"order":1,"harness_template":123}]}
    }}"#;
    let result = TemplateResolver::from_config_str(json);
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert_eq!(result.warnings[0].kind, TemplateWarningKind::EntryInvalid);
    assert!(
        result.warnings[0].message.contains("broken-wf"),
        "告警必须写清是哪条损坏：{}",
        result.warnings[0].message
    );

    // 损坏工作流被丢弃
    assert!(result.resolver.user_template("broken-wf", 1).is_none());
    // 其他工作流正常生效
    assert_eq!(
        result.resolver.user_template(PRESET_ID, 1),
        Some("有效模板 【{goal}】")
    );
}

/// order=0 的步骤被判非法：该步丢弃 + 告警，不影响同工作流其他步骤。
#[test]
fn order_zero_entry_rejected_with_warning() {
    let json = r#"{"workflows":{"preset-requirement-to-report":{"steps":[
        {"order":0,"harness_template":"非法序号"},
        {"order":1,"harness_template":"正常模板 【{goal}】"}
    ]}}}"#;
    let result = TemplateResolver::from_config_str(json);
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert_eq!(result.warnings[0].kind, TemplateWarningKind::EntryInvalid);
    assert!(result.resolver.user_template(PRESET_ID, 0).is_none());
    assert_eq!(
        result.resolver.user_template(PRESET_ID, 1),
        Some("正常模板 【{goal}】")
    );
}

/// 同一工作流同一 step order 重复配置 → 后写入的丢弃 + 告警，保留先配置的。
#[test]
fn duplicate_step_order_skips_later_entry() {
    let json = r#"{"workflows":{"preset-requirement-to-report":{"steps":[
        {"order":1,"harness_template":"先配置的模板"},
        {"order":1,"harness_template":"后配置的模板"}
    ]}}}"#;
    let result = TemplateResolver::from_config_str(json);
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    assert_eq!(result.warnings[0].kind, TemplateWarningKind::EntryInvalid);
    assert_eq!(
        result.resolver.user_template(PRESET_ID, 1),
        Some("先配置的模板")
    );
}

/// 空白模板 = 该步未配置：跳过（无告警），回退内置默认。
#[test]
fn blank_template_entry_is_unconfigured() {
    let json = r#"{"workflows":{"preset-requirement-to-report":{"steps":[
        {"order":1,"harness_template":"   "}
    ]}}}"#;
    let result = TemplateResolver::from_config_str(json);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    let wf = preset();
    let step1 = wf.step(1).expect("步骤 1 必须存在");
    let resolved = result
        .resolver
        .resolve(step1.harness_template.as_deref(), &wf.id, step1.order);
    assert_eq!(resolved.source, TemplateSource::BuiltinDefault);
}

/// 用户配置模板含未知占位符 → 告警（写清工作流/步骤/具体占位符）+ 原样保留不炸。
#[test]
fn unknown_placeholder_warns_and_stays_literal() {
    let json = r#"{"workflows":{"preset-requirement-to-report":{"steps":[
        {"order":1,"harness_template":"【{goal}】请 {bogus} 和 {unknown_x}"}
    ]}}}"#;
    let result = TemplateResolver::from_config_str(json);
    assert!(
        result.warnings.is_empty(),
        "加载阶段不应告警：{:?}",
        result.warnings
    );

    let wf = preset();
    let step1 = wf.step(1).expect("步骤 1 必须存在");
    let rendered = result
        .resolver
        .render_envelope(&wf, step1, "贪吃蛇", Some("planner"));
    // 已知占位符替换、未知占位符原样保留（与 P0 替换语义一致）
    assert_eq!(rendered.text, "【贪吃蛇】请 {bogus} 和 {unknown_x}");

    assert_eq!(rendered.warnings.len(), 1, "{:?}", rendered.warnings);
    let warning = &rendered.warnings[0];
    assert_eq!(warning.kind, TemplateWarningKind::UnknownPlaceholder);
    assert!(
        warning.message.contains("{bogus}") && warning.message.contains("{unknown_x}"),
        "告警必须列全未知占位符：{}",
        warning.message
    );
    assert!(
        warning.message.contains(PRESET_ID) && warning.message.contains("第 1 步"),
        "告警必须写清哪里失败：{}",
        warning.message
    );
}

/// 步骤自带模板含未知占位符 → 同样告警 + 原样保留（坏模板不炸，只影响该信封文案）。
#[test]
fn step_template_unknown_placeholder_warns() {
    let wf = preset();
    let mut step1 = wf.step(1).expect("步骤 1 必须存在").clone();
    step1.harness_template = Some("占位 {goal} {oops}".to_string());

    let resolver = TemplateResolver::new();
    let rendered = resolver.render_envelope(&wf, &step1, "贪吃蛇", Some("planner"));
    assert_eq!(rendered.text, "占位 贪吃蛇 {oops}");
    assert_eq!(rendered.warnings.len(), 1, "{:?}", rendered.warnings);
    assert_eq!(
        rendered.warnings[0].kind,
        TemplateWarningKind::UnknownPlaceholder
    );
}

/// 未注入任何用户模板时，解析器渲染结果与 P0 `render_envelope` 完全一致（向后兼容）。
#[test]
fn resolver_default_render_equals_free_render_envelope() {
    let resolver = TemplateResolver::new();
    let wf = preset();
    for order in 1..=3 {
        let step = wf
            .step(order)
            .unwrap_or_else(|| panic!("步骤 {order} 必须存在"));
        let next_role = if order < 3 { Some("next-role") } else { None };
        let via_resolver = resolver.render_envelope(&wf, step, "贪吃蛇", next_role);
        let via_free = render_envelope(&wf, step, "贪吃蛇", next_role);
        assert_eq!(
            via_resolver.text, via_free,
            "第 {order} 步渲染必须与 P0 一致"
        );
        assert!(
            via_resolver.warnings.is_empty(),
            "{:?}",
            via_resolver.warnings
        );
    }
}

/// 空配置 / 只含空 workflows 的配置 = 未配置任何用户模板（无告警）。
#[test]
fn empty_config_is_noop() {
    for json in ["{}", r#"{"workflows":{}}"#] {
        let result = TemplateResolver::from_config_str(json);
        assert!(result.warnings.is_empty(), "{json}: {:?}", result.warnings);
        assert_eq!(result.resolver.user_templates(), &Default::default());
    }
}
