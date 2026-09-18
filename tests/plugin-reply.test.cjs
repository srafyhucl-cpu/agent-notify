const assert = require("node:assert/strict");
const childProcess = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const repoRoot = path.resolve(__dirname, "..");
const pluginPath = path.resolve(__dirname, "..", "plugin", "agent-notify.ts");
const tscPath = path.join(repoRoot, "node_modules", "typescript", "bin", "tsc");
const testTempRoot =
  process.env.AGENT_NOTIFY_TEST_TEMP_DIR ||
  path.join(path.parse(repoRoot).root, "Temp", "agent-notify-plugin-tests");
const waitIntervalMs = 20;
const waitTimeoutMs = 3000;
const heartbeatSettleMs = 100;
const replyArtifactDirectories = new Set([
  "heartbeats",
  "pending",
  "processing",
  "results",
]);

let compiledPluginBuild;
let compiledPluginPath;

test.after(() => {
  if (!compiledPluginBuild) {
    return;
  }
  const build = compiledPluginBuild;
  compiledPluginBuild = undefined;
  compiledPluginPath = undefined;
  build.cleanup();
});

test.after(() => {
  try {
    fs.rmdirSync(testTempRoot);
  } catch {
    // Other test cleanup still owns files in the shared root.
  }
});

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function waitFor(predicate, description) {
  const deadline = Date.now() + waitTimeoutMs;
  while (Date.now() < deadline) {
    if (predicate()) {
      return;
    }
    await delay(waitIntervalMs);
  }
  throw new Error(`timed out waiting for ${description}`);
}

function loadPlugin() {
  if (!compiledPluginPath) {
    const build = createTemporaryDirectory("agent-notify-plugin-build-");
    try {
      const result = childProcess.spawnSync(
        process.execPath,
        [
          tscPath,
          "--ignoreConfig",
          "--target",
          "ES2022",
          "--module",
          "commonjs",
          "--esModuleInterop",
          "--skipLibCheck",
          "--types",
          "node",
          "--outDir",
          build.directory,
          "--rootDir",
          path.dirname(pluginPath),
          pluginPath,
        ],
        {
          cwd: repoRoot,
          encoding: "utf8",
          windowsHide: true,
        },
      );
      if (result.error) {
        throw result.error;
      }
      if (result.status !== 0) {
        const detail = [result.stdout, result.stderr].filter(Boolean).join("\n").trim();
        throw new Error(`tsc exited with ${result.status}${detail ? `: ${detail}` : ""}`);
      }
      const outputPath = path.join(build.directory, "agent-notify.js");
      if (!fs.existsSync(outputPath)) {
        throw new Error(`tsc did not create ${outputPath}`);
      }
      compiledPluginBuild = build;
      compiledPluginPath = outputPath;
    } catch (error) {
      build.cleanup();
      throw error;
    }
  }
  delete require.cache[compiledPluginPath];
  return require(compiledPluginPath).default;
}

function createPluginContext(prompt, options = {}) {
  const session = {
    context: options.context || (async () => ({})),
    get: options.get || (async () => ({})),
  };
  let clientSession = session;
  const promptAPI = options.promptAPI || "current";
  if (prompt && promptAPI === "current") {
    session.prompt = prompt;
  } else if (prompt && promptAPI === "legacy") {
    clientSession = { ...session, promptAsync: prompt };
  } else if (prompt && promptAPI === "direct") {
    session.promptAsync = prompt;
  }
  const events = options.events || [];
  return {
    event: {
      async *subscribe({ signal }) {
        for (const event of events) {
          yield event;
        }
        await new Promise((resolve) => {
          signal.addEventListener("abort", resolve, { once: true });
        });
      },
    },
    session,
    client: options.withClient === false ? undefined : { session: clientSession },
  };
}

function createReplyJob(id, overrides = {}) {
  const now = Date.now();
  return {
    id,
    sessionID: "session-1",
    text: "continue",
    createdAt: new Date(now).toISOString(),
    expiresAt: new Date(now + 60_000).toISOString(),
    ...overrides,
  };
}

// createGatedEventContext 构造一个可精确控制时序的插件上下文：
// 每调用一次 release()，事件流才产出下一个 session.execution.succeeded 事件。
function createGatedEventContext(sessionIDs) {
  const waiters = [];
  const subscribe = async function* ({ signal }) {
    for (let index = 0; index < sessionIDs.length; index++) {
      await new Promise((resolve) => {
        waiters.push(resolve);
      });
      yield {
        type: "session.execution.succeeded",
        properties: { sessionID: sessionIDs[index] },
      };
    }
    await new Promise((resolve) => {
      signal.addEventListener("abort", resolve, { once: true });
    });
  };
  return {
    context: {
      event: { subscribe },
      session: {
        context: async () => ({}),
        get: async () => ({}),
        prompt: async () => ({}),
      },
      client: { session: { promptAsync: async () => ({}) } },
    },
    release() {
      const resolve = waiters.shift();
      if (resolve) {
        resolve();
      }
    },
  };
}

function writeAtomic(target, value) {
  fs.mkdirSync(path.dirname(target), { recursive: true, mode: 0o700 });
  const temporary = `${target}.tmp-${process.pid}`;
  fs.writeFileSync(temporary, JSON.stringify(value), { mode: 0o600 });
  fs.renameSync(temporary, target);
}

function writeReplyJob(replyDir, job, state = "pending") {
  writeAtomic(path.join(replyDir, state, `${job.id}.json`), job);
}

function readResult(replyDir, jobID) {
  const resultPath = path.join(replyDir, "results", `${jobID}.json`);
  if (!fs.existsSync(resultPath)) {
    return undefined;
  }
  return JSON.parse(fs.readFileSync(resultPath, "utf8"));
}

function heartbeatFiles(replyDir) {
  const directory = path.join(replyDir, "heartbeats");
  if (!fs.existsSync(directory)) {
    return [];
  }
  return fs.readdirSync(directory).filter((name) => name.endsWith(".json")).sort();
}

function removeFlatDirectory(directory) {
  for (const name of fs.readdirSync(directory)) {
    const target = path.join(directory, name);
    const info = fs.lstatSync(target);
    if (!info.isFile()) {
      throw new Error(`refusing to remove nested path: ${target}`);
    }
    fs.unlinkSync(target);
  }
  fs.rmdirSync(directory);
}

function createTemporaryDirectory(prefix, allowedDirectories = new Set()) {
  fs.mkdirSync(testTempRoot, { recursive: true });
  const root = fs.realpathSync(testTempRoot);
  const directory = fs.mkdtempSync(path.join(root, prefix));
  return {
    directory,
    cleanup() {
      const resolved = fs.realpathSync(directory);
      if (path.dirname(resolved) !== root || !path.basename(resolved).startsWith(prefix)) {
        throw new Error(`refusing to clean unexpected path: ${resolved}`);
      }
      for (const name of fs.readdirSync(resolved)) {
        const target = path.join(resolved, name);
        const info = fs.lstatSync(target);
        if (info.isFile()) {
          fs.unlinkSync(target);
          continue;
        }
        if (info.isDirectory() && allowedDirectories.has(name)) {
          removeFlatDirectory(target);
          continue;
        }
        throw new Error(`refusing to remove unexpected path: ${target}`);
      }
      fs.rmdirSync(resolved);
    },
  };
}

function withEnvironmentVars(values) {
  const previous = new Map();
  for (const [key, value] of Object.entries(values)) {
    previous.set(key, process.env[key]);
    if (value === undefined) {
      delete process.env[key];
    } else {
      process.env[key] = String(value);
    }
  }
  return () => {
    for (const [key, value] of previous) {
      if (value === undefined) {
        delete process.env[key];
      } else {
        process.env[key] = value;
      }
    }
  };
}

function withEnvironment(replyDir, replyTimeoutMs) {
  return withEnvironmentVars({
    AGENT_NOTIFY_OPENCODE_REPLY_DIR: replyDir,
    AGENT_NOTIFY_OPENCODE_REPLY_TIMEOUT_MS:
      replyTimeoutMs === undefined ? undefined : replyTimeoutMs,
    AGENT_NOTIFY_DEBUG: undefined,
  });
}

test("successful session prompt is claimed and recorded once", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-success-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  const calls = [];
  let promptThis;
  let dispose;
  try {
    const job = createReplyJob("job-success");
    writeReplyJob(temporary.directory, job);
    const plugin = loadPlugin();
    const context = createPluginContext(async function (input) {
      promptThis = this;
      calls.push(input);
    });
    dispose = await plugin.setup(context);
    await waitFor(
      () => heartbeatFiles(temporary.directory).length === 1,
      "one plugin heartbeat",
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === true,
      "successful result",
    );
    await delay(heartbeatSettleMs);

    assert.equal(calls.length, 1);
    assert.equal(promptThis, context.session);
    assert.deepEqual(calls[0], {
      sessionID: job.sessionID,
      text: job.text,
      delivery: "steer",
    });
    assert.equal(
      fs.existsSync(path.join(temporary.directory, "processing", `${job.id}.json`)),
      false,
    );
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("session fallback uses the direct promptAsync shape", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-session-fallback-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  const calls = [];
  let dispose;
  try {
    const job = createReplyJob("job-session-fallback");
    writeReplyJob(temporary.directory, job);
    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(
        async (input) => {
          calls.push(input);
        },
        { withClient: false, promptAPI: "direct" },
      ),
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === true,
      "direct session result",
    );

    assert.deepEqual(calls, [
      {
        sessionID: job.sessionID,
        parts: [{ type: "text", text: job.text }],
        throwOnError: true,
      },
    ]);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("legacy client promptAsync remains supported", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-legacy-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  const calls = [];
  let promptThis;
  let dispose;
  try {
    const job = createReplyJob("job-legacy");
    writeReplyJob(temporary.directory, job);
    const plugin = loadPlugin();
    const context = createPluginContext(
      async function (input) {
        promptThis = this;
        calls.push(input);
      },
      { promptAPI: "legacy" },
    );
    dispose = await plugin.setup(context);
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === true,
      "legacy client result",
    );

    assert.equal(promptThis, context.client.session);
    assert.deepEqual(calls, [
      {
        path: { id: job.sessionID },
        body: { parts: [{ type: "text", text: job.text }] },
        throwOnError: true,
      },
    ]);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("current session prompt wins over legacy fallback", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-current-priority-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let currentCalls = 0;
  let legacyCalls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-current-priority");
    writeReplyJob(temporary.directory, job);
    const plugin = loadPlugin();
    const context = createPluginContext(async () => {
      currentCalls += 1;
    });
    context.client.session.promptAsync = async () => {
      legacyCalls += 1;
    };
    dispose = await plugin.setup(context);
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === true,
      "current prompt priority result",
    );

    assert.equal(currentCalls, 1);
    assert.equal(legacyCalls, 0);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("completed job result suppresses a duplicate pending file", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-duplicate-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-completed");
    writeReplyJob(temporary.directory, job);
    writeAtomic(
      path.join(temporary.directory, "results", `${job.id}.json`),
      { ok: true, error: "" },
    );

    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
      }),
    );
    await waitFor(
      () => heartbeatFiles(temporary.directory).length === 1,
      "duplicate job heartbeat",
    );
    await waitFor(
      () =>
        fs.existsSync(
          path.join(temporary.directory, "pending", `${job.id}.json`),
        ) === false,
      "duplicate pending cleanup",
    );

    assert.equal(calls, 0);
    assert.equal(readResult(temporary.directory, job.id).ok, true);
    assert.equal(
      fs.existsSync(path.join(temporary.directory, "processing", `${job.id}.json`)),
      false,
    );
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("hung session prompt times out and does not block later jobs", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-prompt-hang-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory, 100);
  let hangingCalls = 0;
  let followUpCalls = 0;
  let rejectHanging;
  let dispose;
  try {
    const hangingJob = createReplyJob("job-a-hanging", { text: "hang" });
    const followUpJob = createReplyJob("job-b-follow-up", { text: "second" });
    writeReplyJob(temporary.directory, hangingJob);
    writeReplyJob(temporary.directory, followUpJob);
    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext((input) => {
        const text = input.text;
        if (text === "hang") {
          hangingCalls += 1;
          return new Promise((_resolve, reject) => {
            rejectHanging = reject;
          });
        }
        followUpCalls += 1;
        return Promise.resolve();
      }),
    );
    await waitFor(
      () => readResult(temporary.directory, hangingJob.id)?.ok === false,
      "prompt timeout result",
    );
    await waitFor(
      () => readResult(temporary.directory, followUpJob.id)?.ok === true,
      "follow-up job result",
    );

    const timedOut = readResult(temporary.directory, hangingJob.id);
    assert.match(timedOut.error, /超时/);
    assert.match(timedOut.error, /未自动重试/);
    assert.equal(hangingCalls, 1);
    assert.equal(followUpCalls, 1);
    assert.equal(
      fs.existsSync(
        path.join(temporary.directory, "processing", `${hangingJob.id}.json`),
      ),
      false,
    );

    // 超时后才到达的拒绝不得变成未处理拒绝，也不得覆盖已记录的结果。
    assert.equal(typeof rejectHanging, "function");
    rejectHanging(new Error("late prompt failure"));
    await delay(heartbeatSettleMs);
    assert.equal(hangingCalls, 1);
    assert.equal(readResult(temporary.directory, hangingJob.id).error, timedOut.error);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("session prompt failure is recorded without replay", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-failure-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-failure");
    writeReplyJob(temporary.directory, job);
    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
        throw new Error("session missing");
      }),
    );
    await waitFor(
      () => heartbeatFiles(temporary.directory).length === 1,
      "one plugin heartbeat",
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === false,
      "failed result",
    );
    await delay(heartbeatSettleMs);

    const result = readResult(temporary.directory, job.id);
    assert.match(result.error, /session missing/);
    assert.equal(calls, 1);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("empty session prompt error uses nested cause details", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-nested-error-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-nested-error");
    writeReplyJob(temporary.directory, job);
    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
        throw new Error("", { cause: { status: 404 } });
      }),
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === false,
      "nested error result",
    );
    await delay(heartbeatSettleMs);

    assert.equal(readResult(temporary.directory, job.id).error, "HTTP 404");
    assert.equal(calls, 1);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("nested error object keeps its protocol tag", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-error-tag-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-error-tag");
    writeReplyJob(temporary.directory, job);
    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
        throw { error: { _tag: "SessionNotFoundError" } };
      }),
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === false,
      "tagged error result",
    );
    await delay(heartbeatSettleMs);

    assert.match(readResult(temporary.directory, job.id).error, /SessionNotFoundError/);
    assert.equal(calls, 1);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("resolved session prompt error is recorded without replay", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-result-error-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-result-error");
    writeReplyJob(temporary.directory, job);
    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
        return { error: { message: "session missing" } };
      }),
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === false,
      "resolved error result",
    );
    await delay(heartbeatSettleMs);

    assert.match(readResult(temporary.directory, job.id).error, /session missing/);
    assert.equal(calls, 1);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("session prompt failure redacts reply text", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-redaction-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  const sensitiveText = "private reply text";
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-redaction", { text: sensitiveText });
    writeReplyJob(temporary.directory, job);
    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
        throw new Error(`prompt failed: ${sensitiveText}`);
      }),
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === false,
      "redacted failure result",
    );
    const result = readResult(temporary.directory, job.id);
    assert.match(result.error, /\[REDACTED\]/);
    assert.equal(result.error.includes(sensitiveText), false);
    assert.equal(calls, 1);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("session prompt failure redacts JSON-escaped reply text", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-redaction-json-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  const sensitiveText = 'private "reply"\nsecond line';
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-redaction-json", { text: sensitiveText });
    writeReplyJob(temporary.directory, job);
    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
        throw new Error(`prompt failed: ${JSON.stringify({ text: sensitiveText })}`);
      }),
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === false,
      "JSON-redacted failure result",
    );
    const result = readResult(temporary.directory, job.id);
    const escaped = JSON.stringify(sensitiveText).slice(1, -1);
    assert.match(result.error, /\[REDACTED\]/);
    assert.equal(result.error.includes(sensitiveText), false);
    assert.equal(result.error.includes(escaped), false);
    assert.equal(calls, 1);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("unsupported plugin does not claim pending jobs", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-unsupported-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let dispose;
  try {
    const job = createReplyJob("job-unsupported");
    writeReplyJob(temporary.directory, job);
    const plugin = loadPlugin();
    dispose = await plugin.setup(createPluginContext());
    await waitFor(
      () => heartbeatFiles(temporary.directory).length === 1,
      "unsupported heartbeat",
    );
    await delay(heartbeatSettleMs);

    assert.equal(
      fs.existsSync(path.join(temporary.directory, "pending", `${job.id}.json`)),
      true,
    );
    assert.equal(readResult(temporary.directory, job.id), undefined);
    assert.equal(
      fs.existsSync(path.join(temporary.directory, "processing", `${job.id}.json`)),
      false,
    );
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("expired pending job is removed by an unsupported plugin", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-expired-pending-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let dispose;
  try {
    const job = createReplyJob("job-expired-pending", {
      expiresAt: new Date(Date.now() - 60_000).toISOString(),
    });
    writeReplyJob(temporary.directory, job);
    const pendingPath = path.join(
      temporary.directory,
      "pending",
      `${job.id}.json`,
    );
    const plugin = loadPlugin();
    dispose = await plugin.setup(createPluginContext());
    await waitFor(
      () => heartbeatFiles(temporary.directory).length === 1,
      "unsupported heartbeat",
    );
    await waitFor(
      () => fs.existsSync(pendingPath) === false,
      "expired pending job cleanup",
    );

    assert.equal(readResult(temporary.directory, job.id), undefined);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("stale processing job is failed without replay", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-stale-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-stale");
    writeReplyJob(temporary.directory, job, "processing");
    const processingPath = path.join(
      temporary.directory,
      "processing",
      `${job.id}.json`,
    );
    const old = new Date(Date.now() - 60_000);
    fs.utimesSync(processingPath, old, old);

    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
      }),
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === false,
      "interrupted job result",
    );

    assert.equal(calls, 0);
    assert.match(readResult(temporary.directory, job.id).error, /未自动重试/);
    assert.equal(fs.existsSync(processingPath), false);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("expired job is failed even when its owner heartbeat is live", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-expired-owned-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-expired-owned", {
      owner: "owner-expired",
      expiresAt: new Date(Date.now() - 60_000).toISOString(),
    });
    writeReplyJob(temporary.directory, job, "processing");
    const processingPath = path.join(
      temporary.directory,
      "processing",
      `${job.id}.json`,
    );
    writeAtomic(
      path.join(temporary.directory, "heartbeats", "owner-expired.json"),
      { ready: true, timestamp: new Date().toISOString() },
    );

    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
      }),
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === false,
      "expired owned job result",
    );

    assert.equal(calls, 0);
    assert.equal(
      readResult(temporary.directory, job.id).error,
      "引用回复任务已过期且未确认，未自动重试",
    );
    assert.equal(fs.existsSync(processingPath), false);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("live owner heartbeat prevents stale recovery", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-owner-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-owned", { owner: "owner-1" });
    writeReplyJob(temporary.directory, job, "processing");
    const processingPath = path.join(
      temporary.directory,
      "processing",
      `${job.id}.json`,
    );
    const old = new Date(Date.now() - 60_000);
    fs.utimesSync(processingPath, old, old);
    writeAtomic(
      path.join(temporary.directory, "heartbeats", "owner-1.json"),
      { ready: true, timestamp: new Date().toISOString() },
    );

    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
      }),
    );
    await waitFor(
      () => heartbeatFiles(temporary.directory).length === 2,
      "owner and current plugin heartbeats",
    );
    await delay(heartbeatSettleMs);

    assert.equal(calls, 0);
    assert.equal(fs.existsSync(processingPath), true);
    assert.equal(readResult(temporary.directory, job.id), undefined);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("future owner heartbeat does not prevent stale recovery", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-owner-future-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-owned-future", { owner: "owner-future" });
    writeReplyJob(temporary.directory, job, "processing");
    const processingPath = path.join(
      temporary.directory,
      "processing",
      `${job.id}.json`,
    );
    const old = new Date(Date.now() - 60_000);
    fs.utimesSync(processingPath, old, old);
    writeAtomic(
      path.join(temporary.directory, "heartbeats", "owner-future.json"),
      { ready: true, timestamp: new Date(Date.now() + 60_000).toISOString() },
    );

    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
      }),
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === false,
      "future-skewed job recovery",
    );

    assert.equal(calls, 0);
    assert.match(readResult(temporary.directory, job.id).error, /未自动重试/);
    assert.equal(fs.existsSync(processingPath), false);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("unready owner heartbeat does not block stale recovery", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-owner-unready-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let calls = 0;
  let dispose;
  try {
    const job = createReplyJob("job-owned-unready", { owner: "owner-unready" });
    writeReplyJob(temporary.directory, job, "processing");
    const processingPath = path.join(
      temporary.directory,
      "processing",
      `${job.id}.json`,
    );
    const old = new Date(Date.now() - 60_000);
    fs.utimesSync(processingPath, old, old);
    writeAtomic(
      path.join(temporary.directory, "heartbeats", "owner-unready.json"),
      { ready: false, timestamp: new Date().toISOString() },
    );

    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(async () => {
        calls += 1;
      }),
    );
    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === false,
      "unready-owner job recovery",
    );

    assert.equal(calls, 0);
    assert.match(readResult(temporary.directory, job.id).error, /未自动重试/);
    assert.equal(fs.existsSync(processingPath), false);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("dispose removes only the current instance heartbeat", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-heartbeats-",
    replyArtifactDirectories,
  );
  const restoreEnvironment = withEnvironment(temporary.directory);
  let first;
  let second;
  try {
    const plugin = loadPlugin();
    first = await plugin.setup(createPluginContext(async () => ({})));
    second = await plugin.setup(createPluginContext(async () => ({})));
    await waitFor(
      () => heartbeatFiles(temporary.directory).length === 2,
      "two plugin heartbeats",
    );
    const installed = heartbeatFiles(temporary.directory);
    first();
    const remaining = heartbeatFiles(temporary.directory);
    assert.equal(remaining.length, 1);
    assert.equal(installed.includes(remaining[0]), true);
    assert.equal(
      fs.existsSync(path.join(temporary.directory, "heartbeats", installed[0])),
      false,
    );
  } finally {
    if (first) {
      first();
    }
    if (second) {
      second();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

test("hung session metadata still delivers the notification", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-metadata-hang-",
    replyArtifactDirectories,
  );
  const sessionID = "session-hanging-metadata";
  const debugLogPath = path.join(temporary.directory, "opencode-debug.log");
  const readDebugLog = () => {
    try {
      return fs.readFileSync(debugLogPath, "utf8");
    } catch {
      return "";
    }
  };
  const restoreEnvironment = withEnvironmentVars({
    AGENT_NOTIFY_OPENCODE_REPLY_DIR: temporary.directory,
    AGENT_NOTIFY_OPENCODE_MARKER_FILE: path.join(temporary.directory, "opencode.off"),
    AGENT_NOTIFY_OPENCODE_FETCH_TIMEOUT_MS: 100,
    AGENT_NOTIFY_TEMP_DIR: temporary.directory,
    AGENT_NOTIFY_CONFIG_FILE: path.join(temporary.directory, "config.json"),
    AGENT_NOTIFY_BIN: path.join(temporary.directory, "missing-agent-notify.exe"),
    AGENT_NOTIFY_DEBUG: "1",
  });
  let dispose;
  try {
    const plugin = loadPlugin();
    dispose = await plugin.setup(
      createPluginContext(undefined, {
        events: [{ type: "session.execution.succeeded", properties: { sessionID } }],
        context: () => new Promise(() => {}),
        get: () => new Promise(() => {}),
      }),
    );

    await waitFor(
      () => readDebugLog().includes(`pushed sid=${sessionID}`),
      "notification delivery after metadata timeout",
    );
    const log = readDebugLog();
    assert.match(log, new RegExp(`title fail sid=${sessionID} err=读取会话标题超时`));
    assert.match(log, new RegExp(`summary fail sid=${sessionID} err=读取会话摘要超时`));
    assert.match(log, new RegExp(`spawn sid=${sessionID}`));
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

// 冷却值每次都重新读取：改了环境变量/配置后无需重启插件即可生效（不再缓存）。
test("cooldown change takes effect without restarting the plugin", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-cooldown-",
    replyArtifactDirectories,
  );
  const debugLogPath = path.join(temporary.directory, "opencode-debug.log");
  const readLog = () => {
    try {
      return fs.readFileSync(debugLogPath, "utf8");
    } catch {
      return "";
    }
  };
  const sessionID = "session-cooldown";
  const restoreEnvironment = withEnvironmentVars({
    AGENT_NOTIFY_OPENCODE_REPLY_DIR: temporary.directory,
    AGENT_NOTIFY_OPENCODE_MARKER_FILE: path.join(temporary.directory, "opencode.off"),
    AGENT_NOTIFY_TEMP_DIR: temporary.directory,
    AGENT_NOTIFY_CONFIG_FILE: path.join(temporary.directory, "config.json"),
    AGENT_NOTIFY_BIN: path.join(temporary.directory, "missing-agent-notify.exe"),
    AGENT_NOTIFY_DEBUG: "1",
    AGENT_NOTIFY_OFF: undefined,
    AGENT_NOTIFY_DRYRUN: undefined,
    // 0.001 分钟 = 60ms，方便在测试里跨过冷却窗口。
    AGENT_NOTIFY_COOLDOWN_MIN: "0.001",
  });
  const gated = createGatedEventContext([sessionID, sessionID]);
  let dispose;
  try {
    const plugin = loadPlugin();
    dispose = await plugin.setup(gated.context);

    gated.release();
    await waitFor(() => readLog().includes(`pushed sid=${sessionID}`), "first push");
    await delay(200); // 跨过 60ms 冷却

    process.env.AGENT_NOTIFY_COOLDOWN_MIN = "60";
    gated.release();
    await waitFor(
      () => readLog().includes(`skip: cooldown sid=${sessionID}`),
      "cooldown applied after change",
    );

    const pushes = (readLog().match(new RegExp(`pushed sid=${sessionID}`, "g")) || []).length;
    assert.equal(pushes, 1);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});

// 引用回复提交成功后，该会话的下一次完成事件豁免一次冷却；之后回到正常冷却。
test("successful reply exempts the next completion from cooldown", async () => {
  const temporary = createTemporaryDirectory(
    "agent-notify-plugin-reply-exempt-",
    replyArtifactDirectories,
  );
  const debugLogPath = path.join(temporary.directory, "opencode-debug.log");
  const readLog = () => {
    try {
      return fs.readFileSync(debugLogPath, "utf8");
    } catch {
      return "";
    }
  };
  const sessionID = "session-reply-exempt";
  const restoreEnvironment = withEnvironmentVars({
    AGENT_NOTIFY_OPENCODE_REPLY_DIR: temporary.directory,
    AGENT_NOTIFY_OPENCODE_MARKER_FILE: path.join(temporary.directory, "opencode.off"),
    AGENT_NOTIFY_TEMP_DIR: temporary.directory,
    AGENT_NOTIFY_CONFIG_FILE: path.join(temporary.directory, "config.json"),
    AGENT_NOTIFY_BIN: path.join(temporary.directory, "missing-agent-notify.exe"),
    AGENT_NOTIFY_DEBUG: "1",
    AGENT_NOTIFY_OFF: undefined,
    AGENT_NOTIFY_DRYRUN: undefined,
    AGENT_NOTIFY_COOLDOWN_MIN: "60",
  });
  let dispose;
  try {
    const job = createReplyJob("job-reply-exempt", { sessionID });
    writeReplyJob(temporary.directory, job);
    const gated = createGatedEventContext([sessionID, sessionID]);
    const plugin = loadPlugin();
    dispose = await plugin.setup(gated.context);

    await waitFor(
      () => readResult(temporary.directory, job.id)?.ok === true,
      "reply submission result",
    );

    gated.release();
    await waitFor(() => readLog().includes(`pushed sid=${sessionID}`), "reply-exempt push");
    assert.match(readLog(), new RegExp(`reply-exempt sid=${sessionID}`));

    // 第二次完成事件回到正常冷却：60 分钟窗口内应被跳过。
    gated.release();
    await waitFor(
      () => readLog().includes(`skip: cooldown sid=${sessionID}`),
      "cooldown restored after exemption",
    );
    const pushes = (readLog().match(new RegExp(`pushed sid=${sessionID}`, "g")) || []).length;
    assert.equal(pushes, 1);
  } finally {
    if (dispose) {
      dispose();
    }
    restoreEnvironment();
    temporary.cleanup();
  }
});
