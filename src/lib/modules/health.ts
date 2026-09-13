import { invoke } from "@tauri-apps/api/core";
import { taskInvoke } from "$lib/core/tasks.svelte";

export interface HealthConfig {
	alertsEnabled: boolean;
	recoveryEnabled: boolean;
	cpuThresholdPercent: number;
	memoryThresholdPercent: number;
	minimumFreeDiskGb: number;
	diskPath: string;
	sustainedSeconds: number;
	alertCooldownSeconds: number;
	recoveryBackoffSeconds: number;
}

export interface HealthEvent {
	id: number;
	timestamp: number;
	level: "info" | "warn" | "error";
	kind: string;
	message: string;
	workspaceId: string;
}

export interface HealthSample {
	timestamp: number;
	running: boolean | null;
	pid: number | null;
	cpuPercent: number | null;
	memoryPercent: number | null;
	freeDiskGb: number | null;
	diskPath?: string | null;
	processError?: string | null;
	diskError?: string | null;
}

export interface HealthStatus {
	workspaceId: string;
	config: HealthConfig;
	sample: HealthSample | null;
	events: HealthEvent[];
	recoveryArmed: boolean;
	recoveryBlocked: boolean;
	recoveryAttempts: number;
	nextRecoverySeconds: number | null;
}

export const defaultHealthConfig: HealthConfig = {
	alertsEnabled: false,
	recoveryEnabled: false,
	cpuThresholdPercent: 90,
	memoryThresholdPercent: 80,
	minimumFreeDiskGb: 5,
	diskPath: "",
	sustainedSeconds: 15,
	alertCooldownSeconds: 300,
	recoveryBackoffSeconds: 30,
};

export function healthMetricLabel(
	sample: HealthSample | null | undefined,
	metric: "cpuPercent" | "memoryPercent" | "freeDiskGb",
	unavailable = false,
) {
	if (unavailable) return "Unavailable";
	if (!sample) return "Waiting for sample";
	if (metric !== "freeDiskGb") {
		if (sample.running === false) return "Server stopped";
		if (sample.running == null) return "Unavailable";
	}
	const value = sample[metric];
	if (value == null || !Number.isFinite(value)) return "Unavailable";
	return `${value.toFixed(1)}${metric === "freeDiskGb" ? " GiB" : "%"}`;
}

export function healthProcessLabel(sample: HealthSample | null | undefined, unavailable = false) {
	if (unavailable) return "Unavailable";
	if (!sample) return "Waiting for sample";
	if (sample.running == null) return "Unavailable";
	if (!sample.running) return "Stopped";
	return sample.pid == null ? "Running" : `Running (${sample.pid})`;
}

export function healthSampleIsStale(sample: HealthSample | null | undefined, now = Date.now()) {
	return sample != null && (!Number.isFinite(sample.timestamp) || now - sample.timestamp > 20_000);
}

export function getHealthStatus() {
	return invoke<HealthStatus>("get_health_status");
}

export function configureHealth(config: HealthConfig, workspaceId: string) {
	return taskInvoke<HealthStatus>("configure_health", { config, workspaceId });
}

export function clearHealthEvents() {
	return taskInvoke<void>("clear_health_events");
}
