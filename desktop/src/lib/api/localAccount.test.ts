import { describe, expect, it } from "vitest";
import { validateNewLocalAccount } from "./localAccount";

describe("validateNewLocalAccount", () => {
  it("accepts a plain account", () => {
    expect(validateNewLocalAccount("camille", "mot de passe", "mot de passe")).toBeNull();
    expect(validateNewLocalAccount("  Léa_2 ", "mot de passe", "mot de passe")).toBeNull();
  });

  it("checks the username first", () => {
    expect(validateNewLocalAccount("ab", "mot de passe", "mot de passe")).toMatch(/identifiant/);
    expect(validateNewLocalAccount("x".repeat(33), "mot de passe", "mot de passe")).toMatch(/identifiant/);
    expect(validateNewLocalAccount("camille martin", "mot de passe", "mot de passe")).toMatch(/lettres/);
    expect(validateNewLocalAccount("camille@example.fr", "mot de passe", "mot de passe")).toMatch(/lettres/);
  });

  it("checks the password length and the confirmation", () => {
    expect(validateNewLocalAccount("camille", "court", "court")).toMatch(/8 à 128/);
    expect(validateNewLocalAccount("camille", "x".repeat(129), "x".repeat(129))).toMatch(/8 à 128/);
    expect(validateNewLocalAccount("camille", "mot de passe", "autre chose")).toMatch(/correspondent/);
  });
});
