import { describe, expect, it } from "vitest";
import { STUCK, watchStartup, type StartupChecks } from "./startupWatch";
import type { StartupFailure } from "$lib/api/startup";

const tooNew: StartupFailure = { kind: "database_too_new", title: "t", message: "m" };

function checks(over: Partial<StartupChecks>): StartupChecks {
  return {
    healthy: async () => false,
    recorded: async () => null,
    sleep: async () => {},
    ...over,
  };
}

describe("watchStartup", () => {
  it("is fine as soon as the service answers", async () => {
    expect(await watchStartup(checks({ healthy: async () => true }))).toBeNull();
  });

  it("waits through a slow start", async () => {
    let calls = 0;
    const result = await watchStartup(checks({ healthy: async () => ++calls > 5 }));
    expect(result).toBeNull();
  });

  it("reports the failure the shell recorded", async () => {
    expect(await watchStartup(checks({ recorded: async () => tooNew }))).toEqual(tooNew);
  });

  it("calls a service that never answers stuck, once the budget is spent", async () => {
    expect(await watchStartup(checks({}), 2_000)).toEqual(STUCK);
  });

  it("keeps waiting when asking the shell fails", async () => {
    let calls = 0;
    const result = await watchStartup(
      checks({
        healthy: async () => ++calls > 3,
        recorded: async () => {
          throw new Error("no shell");
        },
      }),
    );
    expect(result).toBeNull();
  });
});
