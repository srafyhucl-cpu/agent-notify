import type {
  AdvanceOrcTaskPayload,
  AgentDto,
  BeginChannelLoginPayload,
  BeginChannelLoginResultDto,
  BusinessCommand,
  ChannelAccountDto,
  ChannelDto,
  CommandError,
  CreateOrcTaskPayload,
  CurrentOrcWorkflowDto,
  DeliveryDto,
  DiagnosticsDto,
  HostEvent,
  InstallUpdateResultDto,
  LoginSessionDto,
  LegacyMigrationDto,
  MarkBlockedOrcTaskPayload,
  MutationAcceptedDto,
  NotificationDetailDto,
  NotificationFilterPayload,
  NotificationListDto,
  NotificationSummaryDto,
  OpencodeModelDto,
  OpencodeProjectDto,
  OrcTaskDto,
  OrcTaskIdPayload,
  OrcTemplateDto,
  OrcWorkflowDto,
  RuntimeSnapshotDto,
  RuntimeSummaryDto,
  SaveOrcTemplateConfigPayload,
  SettingsDto,
  UpdateStatusDto,
} from "./types";
import type {
  CommandPayloadMap,
  CommandResultMap,
  EventPayloadMap,
  HostBridge,
} from "./hostBridge";

export interface BridgeInvocation {
  command: BusinessCommand;
  payload: unknown;
}

export interface MockHostBridgeOptions {
  agents?: AgentDto[];
  channels?: ChannelDto[];
  snapshot?: RuntimeSnapshotDto;
  notifications?: NotificationSummaryDto[];
  notificationDetails?: Record<string, NotificationDetailDto>;
  settings?: SettingsDto;
  diagnostics?: DiagnosticsDto;
  migration?: LegacyMigrationDto;
  updateStatus?: UpdateStatusDto;
  installUpdateResult?: InstallUpdateResultDto;
  loginSession?: LoginSessionDto;
  errors?: Partial<Record<BusinessCommand, CommandError>>;
  delays?: Partial<Record<BusinessCommand, number>>;
  /** 测试发送的投递结果；默认 Sent（用于断言"失败/待送达"的 UI 分支）。 */
  sendTestDelivery?: DeliveryDto;
  /** 预置的编排任务（集群页测试用）。 */
  orcTasks?: OrcTaskDto[];
  /** 当前工作流节点（旧 `get_current_orc_workflow` 兼容命令用）；缺省 = 多 Agent 委托预置。 */
  orcWorkflow?: OrcWorkflowDto;
  /** 编排模板（设置页节点配置与创建任务预览）；缺省 = 内置三档模板（未配置 Agent/模型）。 */
  orcTemplates?: OrcTemplateDto[];
  /** OpenCode 已知项目（工作目录下拉，按最近活跃倒序）；缺省 = 两条固定样本。 */
  opencodeProjects?: OpencodeProjectDto[];
  /** OpenCode 可用模型（模型下拉）；缺省 = 三条固定样本。 */
  opencodeModels?: OpencodeModelDto[];
}

export interface MockHostBridge extends HostBridge {
  calls(command?: BusinessCommand): BridgeInvocation[];
  emit<TEvent extends HostEvent>(
    event: TEvent,
    payload: EventPayloadMap[TEvent],
  ): void;
}

const MOCK_QR_PAYLOAD =
  `data:image/svg+xml;charset=utf-8,${encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="180" height="180" viewBox="0 0 21 21"><rect width="21" height="21" fill="#fff"/><path fill="#111" d="M1 1h7v7H1zm2 2v3h3V3zm10-2h7v7h-7zm2 2v3h3V3zM1 13h7v7H1zm2 2v3h3v-3zm8-2h2v2h-2zm4 1h2v2h-2zm2-1h2v2h-2zm-6 4h2v2h-2zm3 1h2v2h-2zm3 0h3v3h-3zm-7-9h2v2h-2zm4 0h2v3h-2zm-5 4h3v2H9zm5-1h2v2h-2zm3 1h2v3h-2z"/></svg>',
  )}`;

function runtimeSummary(paused = false): RuntimeSummaryDto {
  return {
    appVersion: "2.0.0-dev.0",
    platform: "windows",
    state: paused ? "Paused" : "Running",
    paused,
  };
}

function defaultSnapshot(): RuntimeSnapshotDto {
  return {
    runtime: runtimeSummary(),
    overview: {
      storage: {
        notificationCount: 0,
        deliveryCount: 0,
        pendingOutboxCount: 0,
        recentError: null,
      },
      agents: [],
      channels: [],
      recentDeliveries: [],
    },
    components: [],
    diagnostics: [],
    migration: defaultMigration(),
  };
}

function defaultMigration(): LegacyMigrationDto {
  return {
    state: "NotConfigured",
    sourceDetected: false,
    reportFile: null,
    report: null,
    error: null,
  };
}

function defaultSettings(): SettingsDto {
  return {
    notificationsPaused: false,
    quietHours: null,
    cooldownSeconds: 0,
    defaultChannelAccountId: null,
    replyEnabled: true,
    deliveryReceiptEnabled: false,
    routeTtlSeconds: 86_400,
    autoStart: false,
    startHidden: false,
    updateChannel: "Stable",
  };
}

function defaultUpdateStatus(): UpdateStatusDto {
  return {
    currentVersion: "2.0.0-dev.0",
    availableVersion: null,
    state: "Unsupported",
    signed: false,
    preview: true,
    message: "当前为 Rust 预览包，未签名且不提供在线安装。",
    checkedAt: null,
  };
}

/** 默认按检查结果返回安装成功，供未显式配置安装结果的测试使用。 */
function defaultInstallUpdateResult(
  status: UpdateStatusDto | undefined,
): InstallUpdateResultDto {
  const version = status?.availableVersion ?? status?.currentVersion ?? "2.0.0-dev.0";
  return {
    state: "ReadyToInstall",
    message: `更新包已校验，安装程序已启动（v${version}）。`,
    installedVersion: version,
    signed: status?.signed ?? false,
    preview: status?.preview ?? true,
  };
}

function defaultChannel(): ChannelDto {
  return {
    id: "test-channel",
    displayName: "测试渠道",
    configSchema: {
      type: "object",
      properties: {},
    },
    capabilities: {
      sendText: true,
      receive: true,
      replyRouting: true,
      editMessage: false,
      attachments: false,
      markdown: false,
      maxTextBytes: null,
      inboundModes: ["LocalEvent"],
    },
    accounts: [],
  };
}

function defaultAgent(): AgentDto {
  return {
    id: "test-agent",
    displayName: "测试 Agent",
    description: "用于桌面 UI 测试的假适配器",
    configSchema: {
      type: "object",
      properties: {},
    },
    capabilities: {
      notify: true,
      resume: true,
      sessionTitle: true,
      hookInstaller: false,
      replyWindow: false,
    },
    enabled: true,
    config: {},
    health: {
      available: true,
      detail: null,
    },
  };
}

function createLoginSession(
  payload: BeginChannelLoginPayload,
  session?: LoginSessionDto,
): LoginSessionDto {
  if (session) {
    return session;
  }
  return {
    id: "login-session-1",
    accountId: null,
    accountKey: payload.accountKey,
    state: "QrReady",
    qrPayload: MOCK_QR_PAYLOAD,
    createdAt: "2026-09-19T00:00:00Z",
    message: "请使用渠道客户端扫码",
    error: null,
  };
}

function accepted(id?: string): MutationAcceptedDto {
  return {
    accepted: true,
    id: id ?? null,
  };
}

/** 测试默认预置工作流：多 Agent 委托（判断→规划→实施），与后端 `Workflow::preset(false)` 同构。 */
function defaultOrcWorkflow(): OrcWorkflowDto {
  return {
    id: "preset-requirement-to-report",
    name: "需求→判断→规划→实施",
    steps: [
      { order: 1, role: "orchestrator", agentHint: "codex", model: null, humanGate: false },
      { order: 2, role: "planner", agentHint: "opencode", model: null, humanGate: false },
      { order: 3, role: "executor", agentHint: "commandcode", model: null, humanGate: false },
    ],
  };
}

/**
 * 测试默认模板：与后端内置三档模板同构（`Workflow::builtin`）。
 * 节点不预置 Agent/模型（§3 不预填），由设置页节点配置填充。
 */
function defaultOrcTemplates(): OrcTemplateDto[] {
  return [
    {
      id: "template-quickfix",
      name: "快速修复",
      steps: [
        { order: 1, role: "executor", agent: null, model: null },
        { order: 2, role: "reviewer", agent: null, model: null },
      ],
    },
    {
      id: "template-standard",
      name: "标准交付",
      steps: [
        { order: 1, role: "planner", agent: null, model: null },
        { order: 2, role: "executor", agent: null, model: null },
        { order: 3, role: "reviewer", agent: null, model: null },
      ],
    },
    {
      id: "template-full",
      name: "完整评估",
      steps: [
        { order: 1, role: "orchestrator", agent: null, model: null },
        { order: 2, role: "planner", agent: null, model: null },
        { order: 3, role: "executor", agent: null, model: null },
        { order: 4, role: "reviewer", agent: null, model: null },
      ],
    },
  ];
}

/** 测试默认 OpenCode 项目：按最近活跃倒序（与后端读取顺序一致）。 */
function defaultOpencodeProjects(): OpencodeProjectDto[] {
  return [
    { directory: "D:/Project/agent-notify", name: "agent-notify", lastActiveAt: 1_760_000_000 },
    { directory: "D:/Project/legacy-demo", name: null, lastActiveAt: 1_750_000_000 },
  ];
}

/** 测试默认 OpenCode 可用模型（模型下拉数据源）。 */
function defaultOpencodeModels(): OpencodeModelDto[] {
  return [
    { providerId: "opencode-go", modelId: "deepseek-v4.1-flash", name: "DeepSeek V4.1 Flash" },
    { providerId: "opencode-go", modelId: "space-bunny-free", name: "Space Bunny Free" },
    { providerId: "opencode", modelId: "mimo-v2.6-flash", name: "MiMo-V2.6-Flash" },
  ];
}

/** 模板 id → 展示名（错误文案与后端保持一致）。 */
const TEMPLATE_NAME_HINTS = "快速修复 / 标准交付 / 完整评估";

function orcTemplateUnknown(templateId: string): never {
  throw {
    code: "orc_template_unknown",
    message: `模板不存在：${templateId}（可选：${TEMPLATE_NAME_HINTS}）`,
    retryable: false,
  };
}

function orcModelInvalid(model: string): never {
  throw {
    code: "orc_model_invalid",
    message: `模型格式应为 provider/model：${model}`,
    retryable: false,
  };
}

/** 模型格式与后端一致：第一个 `/` 前后都非空。 */
function isProviderModel(value: string): boolean {
  const separator = value.indexOf("/");
  if (separator <= 0) {
    return false;
  }
  return (
    value.slice(0, separator).trim().length > 0 &&
    value.slice(separator + 1).trim().length > 0
  );
}

/** 深拷贝 DTO 样本，避免 mock 内部写入污染调用方传入的 fixture。 */
function cloneDto<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

/** 工作目录粗校验：非空且形如路径（真实存在性由后端校验，mock 不猜）。 */
function looksLikeDirectory(path: string): boolean {
  return path.includes("/") || path.includes("\\");
}


function orcTaskNotFound(taskId: string): never {
  throw {
    code: "orc.task_not_found",
    message: `任务不存在：${taskId}`,
    retryable: false,
  };
}

function advanceOrcTaskError(taskId: string): never {
  return orcTaskNotFound(taskId);
}

function deliveryFixture(id: string, accountId: string): DeliveryDto {
  return {
    id,
    notificationId: "notification-1",
    channelId: "test-channel",
    accountId,
    state: "Sent",
    externalMessageId: "external-message-1",
    error: null,
    retryable: false,
    updatedAt: "2026-09-19T00:00:00Z",
  };
}

function replaceAccount(
  channels: ChannelDto[],
  accountId: string,
  enabled: boolean,
): ChannelAccountDto {
  for (const channel of channels) {
    const account = channel.accounts.find((candidate) => candidate.id === accountId);
    if (account) {
      return { ...account, enabled };
    }
  }
  return {
    id: accountId,
    channelId: "test-channel",
    displayName: accountId,
    enabled,
    config: {},
    health: {
      available: enabled,
      stale: false,
      detail: null,
    },
    lastInboundAt: null,
    lastDeliveryAt: null,
  };
}

export function createMockHostBridge(
  options: MockHostBridgeOptions = {},
): MockHostBridge {
  const invocations: BridgeInvocation[] = [];
  const listeners = new Map<HostEvent, Set<(payload: unknown) => void>>();
  const channels = options.channels ?? [defaultChannel()];
  const agents = options.agents ?? [];
  // 深拷贝任务种子：mock 内部写入不应污染调用方 fixture（真实桥每次返回新对象）。
  const orcTasks: OrcTaskDto[] = cloneDto(options.orcTasks ?? []);
  const orcTemplates: OrcTemplateDto[] = cloneDto(
    options.orcTemplates ?? defaultOrcTemplates(),
  );
  const opencodeProjects = cloneDto(
    options.opencodeProjects ?? defaultOpencodeProjects(),
  );
  const opencodeModels = cloneDto(
    options.opencodeModels ?? defaultOpencodeModels(),
  );

  async function applyDelay(command: BusinessCommand) {
    const delay = options.delays?.[command] ?? 0;
    if (delay <= 0) {
      return;
    }
    await new Promise((resolve) => window.setTimeout(resolve, delay));
  }

  async function invoke<TCommand extends BusinessCommand>(
    command: TCommand,
    payload: CommandPayloadMap[TCommand],
  ): Promise<CommandResultMap[TCommand]> {
    invocations.push({ command, payload });
    await applyDelay(command);

    const error = options.errors?.[command];
    if (error) {
      throw { ...error };
    }

    let result: unknown;
    switch (command) {
      case "get_snapshot":
        result = options.snapshot ?? defaultSnapshot();
        break;
      case "list_agents":
        result = agents;
        break;
      case "update_agent_config": {
        const update = payload as CommandPayloadMap["update_agent_config"];
        const current =
          agents.find((agent) => agent.id === update.agentId) ?? defaultAgent();
        result = {
          ...current,
          enabled: update.enabled ?? current.enabled,
          config: update.config ?? current.config,
        } satisfies AgentDto;
        break;
      }
      case "list_channel_accounts":
        result = { channels };
        break;
      case "begin_channel_login": {
        const login = payload as BeginChannelLoginPayload;
        result = {
          channelId: login.channelId,
          session: createLoginSession(login, options.loginSession),
        } satisfies BeginChannelLoginResultDto;
        break;
      }
      case "submit_channel_login_code":
        result = {
          ...createLoginSession(
            { channelId: "test-channel", accountKey: "account-key-1" },
            options.loginSession,
          ),
          state: "WaitingFirstInbound",
          qrPayload: null,
          message: "验证码已提交，等待首条入站消息",
        } satisfies LoginSessionDto;
        break;
      case "logout_channel_account": {
        const account = payload as CommandPayloadMap["logout_channel_account"];
        result = accepted(account.accountId);
        break;
      }
      case "enable_channel_account": {
        const account = payload as CommandPayloadMap["enable_channel_account"];
        result = replaceAccount(channels, account.accountId, true);
        break;
      }
      case "disable_channel_account": {
        const account = payload as CommandPayloadMap["disable_channel_account"];
        result = replaceAccount(channels, account.accountId, false);
        break;
      }
      case "send_test_notification": {
        const test = payload as CommandPayloadMap["send_test_notification"];
        result = {
          accepted: true,
          delivery:
            options.sendTestDelivery ??
            deliveryFixture("delivery-test-1", test.accountId),
        };
        break;
      }
      case "list_notifications":
        result = {
          items: options.notifications ?? [],
          total: options.notifications?.length ?? 0,
          nextCursor: null,
        } satisfies NotificationListDto;
        break;
      case "get_notification_detail": {
        const detail = payload as CommandPayloadMap["get_notification_detail"];
        result = options.notificationDetails?.[detail.notificationId] ?? {
          notification: {
            id: detail.notificationId,
            agentId: "test-agent",
            sessionId: null,
            sessionTitle: null,
            title: "测试通知",
            preview: "测试通知正文",
            occurredAt: "2026-09-19T00:00:00Z",
            deliveryStates: ["Sent"],
          },
          body: "测试通知正文",
          metadata: {},
          deliveries: [deliveryFixture("delivery-test-1", "account-1")],
          routeExists: true,
        } satisfies NotificationDetailDto;
        break;
      }
      case "retry_delivery": {
        const retry = payload as CommandPayloadMap["retry_delivery"];
        result = deliveryFixture(retry.deliveryId, "account-1");
        break;
      }
      case "get_diagnostics":
        result =
          options.diagnostics ??
          ({
            generatedAt: "2026-09-19T00:00:00Z",
            runtime: runtimeSummary(),
            storage: {
              notificationCount: 0,
              deliveryCount: 0,
              pendingOutboxCount: 0,
              recentError: null,
            },
            components: [],
            items: [],
            migration: options.migration ?? defaultMigration(),
          } satisfies DiagnosticsDto);
        break;
      case "retry_legacy_migration":
        result = options.migration ?? defaultMigration();
        break;
      case "get_settings":
        result = options.settings ?? defaultSettings();
        break;
      case "update_settings":
        result = payload as SettingsDto;
        break;
      case "set_runtime_paused": {
        const pause = payload as CommandPayloadMap["set_runtime_paused"];
        result = runtimeSummary(pause.paused);
        break;
      }
      case "quit_app":
        result = accepted();
        break;
      case "get_update_status":
        result = options.updateStatus ?? defaultUpdateStatus();
        break;
      case "install_update":
        result =
          options.installUpdateResult ??
          defaultInstallUpdateResult(options.updateStatus);
        break;
      case "create_orc_task": {
        const create = payload as CreateOrcTaskPayload;
        const goal = create.goal.trim();
        if (!goal) {
          throw { code: "orc_goal_empty", message: "任务目标不能为空", retryable: false };
        }
        const template = orcTemplates.find(
          (candidate) => candidate.id === create.templateId.trim(),
        );
        if (!template) {
          orcTemplateUnknown(create.templateId);
        }
        const workingDir = create.workingDir.trim();
        if (!workingDir) {
          throw {
            code: "orc_working_dir_invalid",
            message: "工作目录不能为空：请选择 OpenCode 项目或手动输入目录",
            retryable: false,
          };
        }
        if (!looksLikeDirectory(workingDir)) {
          throw {
            code: "orc_working_dir_invalid",
            message: `工作目录不存在：${workingDir}`,
            retryable: false,
          };
        }
        // 任务级节点配置（新流程，创建即锁定）：order 与模板一致、模型规则、每个节点必须有 Agent。
        let steps = template.steps.map((step) => ({
          order: step.order,
          agent: step.agent?.trim() || null,
          model: step.model?.trim() || null,
        }));
        if (create.steps) {
          if (create.steps.length !== template.steps.length) {
            throw {
              code: "orc_template_steps_invalid",
              message: `模板 ${template.id} 共 ${String(template.steps.length)} 个节点，提交了 ${String(create.steps.length)} 个：请刷新后重试`,
              retryable: false,
            };
          }
          create.steps.forEach((step, index) => {
            const expected = template.steps[index].order;
            if (step.order !== expected) {
              throw {
                code: "orc_template_steps_invalid",
                message: `模板 ${template.id} 第 ${String(index + 1)} 个节点序号应为 ${String(expected)}，实际为 ${String(step.order)}：请刷新后重试`,
                retryable: false,
              };
            }
            const agent = step.agent?.trim() ?? "";
            const model = step.model?.trim() ?? "";
            if (model) {
              if (!isProviderModel(model)) {
                orcModelInvalid(model);
              }
              if (agent !== "opencode") {
                throw {
                  code: "orc_model_agent_unsupported",
                  message: "该 Agent 暂不支持指定模型",
                  retryable: false,
                };
              }
            }
          });
          steps = create.steps.map((step) => ({
            order: step.order,
            agent: step.agent?.trim() || null,
            model: step.model?.trim() || null,
          }));
          const missing = steps.find((step) => step.agent === null);
          if (missing) {
            throw {
              code: "orc_step_agent_missing",
              message: `第 ${String(missing.order)} 步未选择 Agent：请在创建任务时为每个节点选择 Agent`,
              retryable: false,
            };
          }
        }
        const workflow: OrcWorkflowDto = {
          id: template.id,
          name: template.name,
          steps: steps.map((step, index) => ({
            order: step.order,
            role: template.steps[index].role,
            agentHint: step.agent,
            model: step.model,
            humanGate: false,
          })),
        };
        const task: OrcTaskDto = {
          id: `orc-${orcTasks.length + 1}`,
          workflowId: workflow.id,
          state: "working",
          currentStep: 1,
          started: false,
          workflow,
          notifyMode: create.notifyMode ?? "final_only",
          goal,
          workingDir,
          blockedStep: null,
          blockReason: null,
          finalizing: false,
        } satisfies OrcTaskDto;
        orcTasks.push(task);
        result = cloneDto(task);
        break;
      }
      case "list_orc_tasks":
        result = cloneDto(orcTasks);
        break;
      case "start_orc_task": {
        const start = payload as unknown as OrcTaskIdPayload;
        const task = orcTasks.find((item) => item.id === start.taskId);
        if (!task) {
          orcTaskNotFound(start.taskId);
        }
        // 预检：全部节点必须已配置 Agent（后端 §3 语义；缺任一节点即明确报错，任务保持待开始）。
        const missing = task.workflow.steps.find(
          (step) => (step.agentHint ?? "").trim().length === 0,
        );
        if (missing) {
          throw {
            code: "orc_step_agent_missing",
            message: `第 ${missing.order} 步未选择 Agent：请先在设置 → 编排中配置`,
            retryable: false,
          };
        }
        task.started = true;
        task.state = "working";
        result = cloneDto(task);
        break;
      }
      case "get_current_orc_workflow":
        result = {
          workflow: options.orcWorkflow ?? defaultOrcWorkflow(),
        } satisfies CurrentOrcWorkflowDto;
        break;
      case "list_orc_templates":
        result = cloneDto(orcTemplates);
        break;
      case "save_orc_template_config": {
        const save = payload as SaveOrcTemplateConfigPayload;
        const templateId = save.templateId.trim();
        const template = orcTemplates.find((candidate) => candidate.id === templateId);
        if (!template) {
          orcTemplateUnknown(templateId);
        }
        if (save.steps.length !== template.steps.length) {
          throw {
            code: "orc_template_steps_invalid",
            message: `模板 ${templateId} 共 ${String(template.steps.length)} 个节点，提交了 ${String(save.steps.length)} 个：请刷新后重试`,
            retryable: false,
          };
        }
        save.steps.forEach((step, index) => {
          const expected = template.steps[index].order;
          if (step.order !== expected) {
            throw {
              code: "orc_template_steps_invalid",
              message: `模板 ${templateId} 第 ${String(index + 1)} 个节点序号应为 ${String(expected)}，实际为 ${String(step.order)}：请刷新后重试`,
              retryable: false,
            };
          }
          const agent = step.agent?.trim() ?? "";
          const model = step.model?.trim() ?? "";
          if (model) {
            if (!isProviderModel(model)) {
              orcModelInvalid(model);
            }
            if (agent !== "opencode") {
              throw {
                code: "orc_model_agent_unsupported",
                message: "该 Agent 暂不支持指定模型",
                retryable: false,
              };
            }
          }
          template.steps[index].agent = agent || null;
          template.steps[index].model = model || null;
        });
        result = cloneDto(orcTemplates);
        break;
      }
      case "list_opencode_projects":
        result = cloneDto(opencodeProjects);
        break;
      case "list_opencode_models":
        result = cloneDto(opencodeModels);
        break;
      case "advance_orc_task": {
        const advance = payload as unknown as AdvanceOrcTaskPayload;
        const task = orcTasks.find((item) => item.id === advance.taskId);
        if (!task) {
          advanceOrcTaskError(advance.taskId ?? "<unknown>");
        }
        if (task.finalizing) {
          throw {
            code: "orc_task_finalizing",
            message: "任务正在等待项目经理汇总，无需手动推进",
            retryable: false,
          };
        }
        const lastOrder = task.workflow.steps.length;
        if (task.currentStep < lastOrder) {
          task.currentStep += 1;
          task.state = "working";
        } else {
          // 最后一步完成不直接结束：进入项目经理汇总阶段（与后端 §4 一致）。
          task.finalizing = true;
          task.state = "working";
        }
        result = cloneDto(task);
        break;
      }
      case "mark_blocked_orc_task": {
        const block = payload as unknown as MarkBlockedOrcTaskPayload;
        const task = orcTasks.find((item) => item.id === block.taskId);
        if (task) {
          task.state = "failed";
          task.blockedStep = block.step;
          task.blockReason = block.reason ?? "投递失败";
        }
        result = task ? cloneDto(task) : orcTaskNotFound(block.taskId);
        break;
      }
      case "recover_blocked_orc_task": {
        const recover = payload as unknown as OrcTaskIdPayload;
        const task = orcTasks.find((item) => item.id === recover.taskId);
        if (task) {
          task.state = "working";
          task.blockedStep = null;
          task.blockReason = null;
        }
        result = task ? cloneDto(task) : orcTaskNotFound(recover.taskId);
        break;
      }
      default: {
        const neverCommand: never = command;
        throw new Error(`未处理命令: ${String(neverCommand)}`);
      }
    }

    return result as CommandResultMap[TCommand];
  }

  return {
    invoke,
    subscribe(event, handler) {
      const eventListeners = listeners.get(event) ?? new Set();
      const untypedHandler = handler as (payload: unknown) => void;
      eventListeners.add(untypedHandler);
      listeners.set(event, eventListeners);
      return () => {
        eventListeners.delete(untypedHandler);
      };
    },
    calls(command) {
      if (!command) {
        return [...invocations];
      }
      return invocations.filter((invocation) => invocation.command === command);
    },
    emit(event, payload) {
      for (const handler of listeners.get(event) ?? []) {
        handler(payload);
      }
    },
  };
}
