#!/usr/bin/env bun
/**
 * Builds affected pull-request crates or the full workspace for Linux release CI.
 */
import { $ } from "bun";

import { affectedCrates, workspaceCrates } from "./affected_crates";

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null;

const pullRequestBase = async (): Promise<string | undefined> => {
  if (process.env.GITHUB_EVENT_NAME !== "pull_request") {
    return undefined;
  }

  const eventPath = process.env.GITHUB_EVENT_PATH;
  if (eventPath === undefined) {
    throw new Error("GITHUB_EVENT_PATH is required for pull requests");
  }

  const event: unknown = await Bun.file(eventPath).json();
  if (!isRecord(event) || !isRecord(event.pull_request) || !isRecord(event.pull_request.base)) {
    throw new Error("GitHub pull-request event has no base SHA");
  }

  const sha = event.pull_request.base.sha;
  if (typeof sha !== "string" || sha.length === 0) {
    throw new Error("GitHub pull-request event has an invalid base SHA");
  }

  return sha;
};

const main = async (): Promise<void> => {
  const base = await pullRequestBase();
  const crates = base === undefined ? await workspaceCrates() : await affectedCrates(base);

  if (crates.length === 0) {
    return;
  }

  const packageArgs = base === undefined
    ? ["--workspace"]
    : crates.flatMap((crate) => ["-p", crate]);

  await $`cargo zigbuild --locked --release --target aarch64-unknown-linux-gnu --target x86_64-unknown-linux-gnu ${packageArgs}`;
};

await main();

