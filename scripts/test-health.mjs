import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { runInNewContext } from "node:vm";
import ts from "typescript";

const source = readFileSync(new URL("../src/lib/modules/health.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
});
const exports = {};
const calls = [];
const invoke = (command, args) => { calls.push({ command, args }); return Promise.resolve(); };
runInNewContext(outputText, {
  exports,
  require(name) {
    if (name === "@tauri-apps/api/core") return { invoke };
    if (name === "$lib/core/tasks.svelte") return { taskInvoke: invoke };
    throw new Error(`Unexpected dependency: ${name}`);
  },
});
const { defaultHealthConfig, healthMetricLabel, healthProcessLabel, healthSampleIsStale } = exports;
const sample = {
  timestamp: 1_800_000_000_000,
  running: true,
  pid: 1234,
  cpuPercent: 12.5,
  memoryPercent: 24,
  freeDiskGb: 42,
  diskPath: "C:\\health-fixture",
  processError: null,
  diskError: null,
};

test("health defaults keep both opt-ins off and use automatic disk selection", () => {
  assert.equal(defaultHealthConfig.alertsEnabled, false);
  assert.equal(defaultHealthConfig.recoveryEnabled, false);
  assert.equal(defaultHealthConfig.diskPath, "");
  assert.equal(calls.length, 0, "Importing health must not configure or enable monitoring");
});

test("passive metrics format independently of alert and recovery configuration", () => {
  assert.equal(healthMetricLabel(sample, "cpuPercent"), "12.5%");
  assert.equal(healthMetricLabel(sample, "memoryPercent"), "24.0%");
  assert.equal(healthMetricLabel(sample, "freeDiskGb"), "42.0 GiB");
  assert.equal(healthProcessLabel(sample), "Running (1234)");
  assert.equal(healthProcessLabel({ ...sample, pid: null }), "Running");
  const { diskPath, processError, diskError, ...legacySample } = sample;
  assert.equal(healthMetricLabel(legacySample, "cpuPercent"), "12.5%");
  assert.equal(healthProcessLabel(legacySample), "Running (1234)");
});

test("initial samples have a useful waiting label", () => {
  for (const initial of [undefined, null]) {
    for (const metric of ["cpuPercent", "memoryPercent", "freeDiskGb"]) {
      assert.equal(healthMetricLabel(initial, metric), "Waiting for sample");
    }
    assert.equal(healthProcessLabel(initial), "Waiting for sample");
  }
});

test("stopped processes do not hide disk readings or show stale CPU and RAM", () => {
  const stopped = { ...sample, running: false, pid: null };
  assert.equal(healthMetricLabel(stopped, "cpuPercent"), "Server stopped");
  assert.equal(healthMetricLabel(stopped, "memoryPercent"), "Server stopped");
  assert.equal(healthMetricLabel(stopped, "freeDiskGb"), "42.0 GiB");
  assert.equal(healthProcessLabel(stopped), "Stopped");
});

test("missing and nonfinite readings are unavailable, not initial or stopped", () => {
  for (const value of [null, undefined, NaN, Infinity]) {
    const unavailable = { ...sample, cpuPercent: value, memoryPercent: value, freeDiskGb: value };
    for (const metric of ["cpuPercent", "memoryPercent", "freeDiskGb"]) {
      assert.equal(healthMetricLabel(unavailable, metric), "Unavailable");
    }
    assert.equal(healthProcessLabel(unavailable), "Running (1234)");
  }
  const unknown = { ...sample, running: null, pid: null };
  assert.equal(healthMetricLabel(unknown, "cpuPercent"), "Unavailable");
  assert.equal(healthMetricLabel(unknown, "freeDiskGb"), "42.0 GiB");
  assert.equal(healthProcessLabel(unknown), "Unavailable");
});

test("failed refreshes hide old values and recover on the next successful refresh", () => {
  for (const metric of ["cpuPercent", "memoryPercent", "freeDiskGb"]) {
    assert.equal(healthMetricLabel(sample, metric, true), "Unavailable");
  }
  assert.equal(healthProcessLabel(sample, true), "Unavailable");
  assert.equal(healthMetricLabel(sample, "cpuPercent", false), "12.5%");
  assert.equal(healthProcessLabel(sample, false), "Running (1234)");
});

test("an unresponsive native monitor cannot leave stale readings looking current", () => {
  assert.equal(healthSampleIsStale(null, sample.timestamp), false);
  assert.equal(healthSampleIsStale(sample, sample.timestamp + 5_000), false);
  assert.equal(healthSampleIsStale(sample, sample.timestamp + 20_000), false);
  assert.equal(healthSampleIsStale(sample, sample.timestamp + 20_001), true);
  assert.equal(healthSampleIsStale({ ...sample, timestamp: NaN }, sample.timestamp), true);
});

test("unavailable disk does not suppress current process metrics", () => {
  const unavailable = { ...sample, freeDiskGb: null, diskPath: null, diskError: "Folder disappeared" };
  assert.equal(healthMetricLabel(unavailable, "freeDiskGb"), "Unavailable");
  assert.equal(healthMetricLabel(unavailable, "cpuPercent"), "12.5%");
  assert.equal(healthProcessLabel(unavailable), "Running (1234)");
});

test("zero is a measured value, not missing data", () => {
  const zero = { ...sample, cpuPercent: 0, memoryPercent: 0, freeDiskGb: 0 };
  assert.equal(healthMetricLabel(zero, "cpuPercent"), "0.0%");
  assert.equal(healthMetricLabel(zero, "memoryPercent"), "0.0%");
  assert.equal(healthMetricLabel(zero, "freeDiskGb"), "0.0 GiB");
});

test("status polling invokes only the read command", async () => {
  calls.length = 0;
  await exports.getHealthStatus();
  await exports.getHealthStatus();
  assert.deepEqual(calls.map(({ command }) => command), ["get_health_status", "get_health_status"]);
  assert.ok(calls.every(({ args }) => args === undefined));
});

test("opting out preserves the selected folder without requiring a disk read", async () => {
  calls.length = 0;
  const config = { ...defaultHealthConfig, diskPath: "C:\\vanished-folder" };
  await exports.configureHealth(config, "default");
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, "configure_health");
  assert.equal(calls[0].args.config.diskPath, config.diskPath);
  assert.equal(calls[0].args.config.recoveryEnabled, false);
  assert.equal(calls[0].args.config.alertsEnabled, false);
});
