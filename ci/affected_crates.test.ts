/**
 * Exercises the deterministic affected-workspace-crate selection rules.
 */
import { describe, expect, test } from "bun:test";

import {
  selectAffectedCrates,
  type SelectionInput,
} from "./affected_crates";

const metadata = (
  packages: SelectionInput["head"]["packages"],
  nodes: SelectionInput["head"]["nodes"],
): SelectionInput["head"] => ({
  packages,
  nodes,
});

describe("selectAffectedCrates", () => {
  test("selects the deepest changed package and its dependents", () => {
    // Arrange
    const head = metadata(
      [
        { id: "app", name: "app", root: "app" },
        { id: "app-macros", name: "app-macros", root: "app/macros" },
        { id: "unused", name: "unused", root: "unused" },
      ],
      [
        { id: "app", dependencies: ["app-macros"], features: [] },
        { id: "app-macros", dependencies: [], features: [] },
        { id: "unused", dependencies: [], features: [] },
      ],
    );

    // Act
    const affected = selectAffectedCrates({
      changedPaths: ["app/macros/src/lib.rs"],
      head,
    });

    // Assert
    expect(affected).toEqual(["app", "app-macros"]);
  });

  test("selects only changed resolved graphs and their dependents for Cargo.lock", () => {
    // Arrange
    const packages = [
      { id: "app", name: "app", root: "app" },
      { id: "shared", name: "shared", root: "shared" },
      { id: "unused", name: "unused", root: "unused" },
    ];
    const base = metadata(packages, [
      { id: "app", dependencies: ["shared"], features: [] },
      { id: "shared", dependencies: ["registry#dep@1"], features: [] },
      { id: "unused", dependencies: ["registry#other@1"], features: [] },
      { id: "registry#dep@1", dependencies: [], features: [] },
      { id: "registry#other@1", dependencies: [], features: [] },
    ]);
    const head = metadata(packages, [
      { id: "app", dependencies: ["shared"], features: [] },
      { id: "shared", dependencies: ["registry#dep@2"], features: [] },
      { id: "unused", dependencies: ["registry#other@1"], features: [] },
      { id: "registry#dep@2", dependencies: [], features: [] },
      { id: "registry#other@1", dependencies: [], features: [] },
    ]);

    // Act
    const affected = selectAffectedCrates({
      changedPaths: ["Cargo.lock"],
      base,
      head,
    });

    // Assert
    expect(affected).toEqual(["app", "shared"]);
  });

  test("does not select crates when a root Cargo input leaves graphs unchanged", () => {
    // Arrange
    const graph = metadata(
      [{ id: "app", name: "app", root: "app" }],
      [{ id: "app", dependencies: ["registry#dep@1"], features: [] }, {
        id: "registry#dep@1",
        dependencies: [],
        features: [],
      }],
    );

    // Act
    const affected = selectAffectedCrates({
      changedPaths: ["Cargo.toml"],
      base: graph,
      head: graph,
    });

    // Assert
    expect(affected).toEqual([]);
  });
});
