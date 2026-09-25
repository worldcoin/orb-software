/**
 * Selects workspace crates affected between a base revision and HEAD for Rust CI.
 */
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { $ } from "bun";

export type WorkspacePackage = {
  name: string;
  cargoPackageId: string;
};

type Package = WorkspacePackage & {
  id: string;
  root: string;
};

type Node = {
  id: string;
  dependencies: string[];
  features: string[];
};

export type Metadata = {
  packages: Package[];
  nodes: Node[];
};

export type SelectionInput = {
  changedPaths: string[];
  head: Metadata;
  base?: Metadata;
};

const rootCargoInputs = new Set(["Cargo.lock", "Cargo.toml"]);

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null;

const requireString = (value: unknown, name: string): string => {
  if (typeof value !== "string") {
    throw new Error("Cargo metadata has an invalid " + name);
  }

  return value;
};

const requireStrings = (value: unknown, name: string): string[] => {
  if (!Array.isArray(value) || !value.every((item) => typeof item === "string")) {
    throw new Error("Cargo metadata has an invalid " + name);
  }

  return value;
};

const cargoMetadata = async (cwd?: string): Promise<Metadata> => {
  const command = cwd === undefined ? $ : $.cwd(cwd);
  const output = await command`cargo metadata --format-version=1 --locked`.quiet();
  const parsed: unknown = output.json();

  if (!isRecord(parsed)) {
    throw new Error("Cargo metadata is not an object");
  }

  const workspaceRoot = requireString(parsed.workspace_root, "workspace_root");
  const workspaceMembers = new Set(
    requireStrings(parsed.workspace_members, "workspace_members"),
  );
  const rawPackages = parsed.packages;
  if (!Array.isArray(rawPackages)) {
    throw new Error("Cargo metadata has an invalid packages list");
  }

  const packages = rawPackages
    .map((value) => {
      if (!isRecord(value)) {
        throw new Error("Cargo metadata has an invalid package");
      }

      return {
        id: requireString(value.id, "package id"),
        name: requireString(value.name, "package name"),
        manifestPath: requireString(value.manifest_path, "manifest path"),
      };
    })
    .filter((pkg) => workspaceMembers.has(pkg.id))
    .map((pkg) => ({
      id: pkg.id,
      name: pkg.name,
      cargoPackageId: pkg.id,
      root: relative(workspaceRoot, dirname(pkg.manifestPath)),
    }));

  if (!isRecord(parsed.resolve) || !Array.isArray(parsed.resolve.nodes)) {
    throw new Error("Cargo metadata has an invalid resolve graph");
  }

  const nodes = parsed.resolve.nodes.map((value) => {
    if (!isRecord(value)) {
      throw new Error("Cargo metadata has an invalid resolve node");
    }

    return {
      id: requireString(value.id, "node id"),
      dependencies: requireStrings(value.dependencies, "node dependencies"),
      features: requireStrings(value.features, "node features"),
    };
  });

  return { packages, nodes };
};

const changedPackageNames = (
  paths: string[],
  packages: Package[],
): Set<string> => {
  const changed = new Set<string>();

  for (const path of paths) {
    const owner = packages
      .filter((pkg) => path === pkg.root || path.startsWith(pkg.root + "/"))
      .sort((left, right) => right.root.length - left.root.length)[0];

    if (owner !== undefined) {
      changed.add(owner.name);
    }
  }

  return changed;
};

const graphSignature = (metadata: Metadata, rootId: string): string => {
  const nodes = new Map(metadata.nodes.map((node) => [node.id, node]));
  const seen = new Set<string>();
  const pending = [rootId];

  while (pending.length > 0) {
    const id = pending.pop();
    if (id === undefined || seen.has(id)) {
      continue;
    }

    seen.add(id);
    const node = nodes.get(id);
    if (node !== undefined) {
      pending.push(...node.dependencies);
    }
  }

  return [...seen]
    .sort()
    .map((id) => {
      const node = nodes.get(id);
      if (node === undefined) {
        return id;
      }

      return [
        id,
        [...node.features].sort().join(","),
        [...node.dependencies].sort().join(","),
      ].join(":");
    })
    .join("|");
};

const metadataChangedPackages = (base: Metadata, head: Metadata): Set<string> => {
  const baseByName = new Map(base.packages.map((pkg) => [pkg.name, pkg]));
  const changed = new Set<string>();

  for (const pkg of head.packages) {
    const previous = baseByName.get(pkg.name);
    if (
      previous === undefined ||
      previous.root !== pkg.root ||
      graphSignature(base, previous.id) !== graphSignature(head, pkg.id)
    ) {
      changed.add(pkg.name);
    }
  }

  return changed;
};

const dependentNames = (changed: Set<string>, metadata: Metadata): string[] => {
  const nameById = new Map(metadata.packages.map((pkg) => [pkg.id, pkg.name]));
  const dependents = new Map<string, string[]>();

  for (const node of metadata.nodes) {
    const dependent = nameById.get(node.id);
    if (dependent === undefined) {
      continue;
    }

    for (const dependencyId of node.dependencies) {
      const dependency = nameById.get(dependencyId);
      if (dependency === undefined) {
        continue;
      }

      const names = dependents.get(dependency) ?? [];
      names.push(dependent);
      dependents.set(dependency, names);
    }
  }

  const pending = [...changed];
  while (pending.length > 0) {
    const name = pending.pop();
    if (name === undefined) {
      continue;
    }

    for (const dependent of dependents.get(name) ?? []) {
      if (!changed.has(dependent)) {
        changed.add(dependent);
        pending.push(dependent);
      }
    }
  }

  return [...changed].sort();
};

/**
 * Returns directly changed workspace packages and every workspace dependent.
 */
export const selectAffectedCrates = (input: SelectionInput): string[] => {
  if (
    input.changedPaths.some(
      (path) => path === "rust-toolchain.toml" || path.startsWith(".cargo/"),
    )
  ) {
    return input.head.packages.map((pkg) => pkg.name).sort();
  }

  const changed = changedPackageNames(input.changedPaths, input.head.packages);
  const rootInputChanged = input.changedPaths.some((path) => rootCargoInputs.has(path));

  if (input.base !== undefined && rootInputChanged) {
    for (const name of metadataChangedPackages(input.base, input.head)) {
      changed.add(name);
    }
  }

  return dependentNames(changed, input.head);
};

const selectedPackages = (
  names: string[],
  packages: Package[],
): WorkspacePackage[] => {
  const byName = new Map(packages.map((pkg) => [pkg.name, pkg]));

  return names.flatMap((name) => {
    const pkg = byName.get(name);
    return pkg === undefined
      ? []
      : [{ name: pkg.name, cargoPackageId: pkg.cargoPackageId }];
  });
};

/** Returns all workspace packages in lexical name order. */
export const workspaceCrates = async (): Promise<WorkspacePackage[]> =>
  (await cargoMetadata()).packages
    .sort((left, right) => left.name.localeCompare(right.name))
    .map((pkg) => ({ name: pkg.name, cargoPackageId: pkg.cargoPackageId }));

/** Returns exact Cargo package arguments for workspace packages. */
export const cargoPackageArgs = (
  packages: WorkspacePackage[],
): string[] => packages.flatMap((pkg) => ["-p", pkg.cargoPackageId]);

/** Returns workspace crates affected between baseRevision and HEAD. */
export const affectedCrates = async (
  baseRevision: string,
): Promise<WorkspacePackage[]> => {
  if (baseRevision.length === 0) {
    throw new Error("A base revision is required");
  }

  await $`git rev-parse --verify ${baseRevision + "^{commit}"}`.quiet();

  const output = await $`git diff --name-only -z --no-renames ${baseRevision + "...HEAD"}`.quiet();
  const changedPaths = output
    .text()
    .split("\0")
    .filter((path) => path.length > 0);
  const head = await cargoMetadata();

  if (!changedPaths.some((path) => rootCargoInputs.has(path))) {
    return selectedPackages(
      selectAffectedCrates({ changedPaths, head }),
      head.packages,
    );
  }

  const worktree = await mkdtemp(join(tmpdir(), "affected-crates-"));
  try {
    await $`git worktree add --detach ${worktree} ${baseRevision}`.quiet();
    const base = await cargoMetadata(worktree);
    return selectedPackages(
      selectAffectedCrates({ changedPaths, base, head }),
      head.packages,
    );
  } finally {
    await $`git worktree remove --force ${worktree}`.nothrow().quiet();
    await rm(worktree, { recursive: true, force: true });
  }
};
