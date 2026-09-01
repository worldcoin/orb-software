#!/usr/bin/env bun
/**
 * Runs Rust CI tests for affected pull-request crates or the full workspace.
 */
import { $ } from "bun";

import {
  affectedCrates,
  cargoPackageArgs,
  type WorkspacePackage,
  workspaceCrates,
} from "./affected_crates";

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
  const candidates = base === undefined ? await workspaceCrates() : await affectedCrates(base);
  const supported: WorkspacePackage[] = [];

  for (const crate of candidates) {
    const result = await $`cargo x crate-supported ${crate.name}`.quiet().nothrow();

    if (result.exitCode === 0) {
      supported.push(crate);
      continue;
    }

    if (result.exitCode === 1) {
      continue;
    }

    throw new Error("cargo x crate-supported failed for " + crate.name);
  }

  if (supported.length === 0) {
    return;
  }

  const packageArgs = cargoPackageArgs(supported);
  await $`cargo nextest run --all-features --all-targets ${packageArgs}`;
};

await main();
