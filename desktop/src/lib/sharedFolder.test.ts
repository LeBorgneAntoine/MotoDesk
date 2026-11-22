import { describe, expect, it } from "vitest";
import {
  auditLabel,
  chunkRanges,
  formatFileSize,
  percent,
  storageSentence,
  syncingSentence,
  visibilityLabel,
} from "./sharedFolder";

describe("chunkRanges", () => {
  it("cuts a file into full chunks and a shorter last one", () => {
    expect(chunkRanges(2_500, 1_000)).toEqual([
      [0, 1_000],
      [1_000, 2_000],
      [2_000, 2_500],
    ]);
  });

  it("makes one chunk of a small file and none of an empty one", () => {
    expect(chunkRanges(10, 1_000)).toEqual([[0, 10]]);
    expect(chunkRanges(1_000, 1_000)).toEqual([[0, 1_000]]);
    expect(chunkRanges(0, 1_000)).toEqual([]);
  });

  it("covers every byte exactly once", () => {
    const size = 3 * 1024 * 1024 + 17;
    const ranges = chunkRanges(size, 1024 * 1024);
    expect(ranges.reduce((n, [a, b]) => n + (b - a), 0)).toBe(size);
    expect(ranges.at(-1)?.[1]).toBe(size);
  });
});

describe("formatFileSize", () => {
  it("reads in the shop's units", () => {
    expect(formatFileSize(0)).toBe("0 octets");
    expect(formatFileSize(1)).toBe("1 octet");
    expect(formatFileSize(820 * 1024)).toBe("820 Ko");
    expect(formatFileSize(1.5 * 1024 * 1024)).toBe("1,5 Mo");
    expect(formatFileSize(100 * 1024 * 1024)).toBe("100 Mo");
  });

  it("does not print a lone decimal zero", () => {
    expect(formatFileSize(2 * 1024 * 1024)).toBe("2 Mo");
  });

  it("copes with nonsense", () => {
    expect(formatFileSize(-1)).toBe("—");
    expect(formatFileSize(Number.NaN)).toBe("—");
  });
});

describe("percent", () => {
  it("stays between 0 and 100", () => {
    expect(percent(0, 10)).toBe(0);
    expect(percent(5, 10)).toBe(50);
    expect(percent(20, 10)).toBe(100);
    expect(percent(0, 0)).toBe(100);
  });
});

describe("labels", () => {
  it("names the visibilities and the audit actions", () => {
    expect(visibilityLabel("admins")).toBe("Administrateurs");
    expect(visibilityLabel("everyone")).toBe("Tous les employés");
    expect(auditLabel("file_opened")).toBe("a ouvert");
    expect(auditLabel("something_new")).toBe("something_new");
  });
});

describe("where the files live", () => {
  it("only claims copies on other PCs when they are made", () => {
    expect(storageSentence(true)).toContain("postes de votre réseau");
    expect(storageSentence(false)).toContain("sur ce poste");
    expect(storageSentence(false)).not.toContain("réseau");
  });

  it("counts the files still arriving", () => {
    expect(syncingSentence(0)).toBe("");
    expect(syncingSentence(1)).toBe("1 fichier est en cours de copie depuis un autre poste.");
    expect(syncingSentence(3)).toBe("3 fichiers sont en cours de copie depuis un autre poste.");
  });
});
