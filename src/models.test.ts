import { describe, expect, it } from "vitest";
import {
  languageMenu,
  modelForLanguage,
  recognizersFor,
  supports,
} from "./models";
import { LANGUAGES, catalog } from "./testModels";

const models = catalog();
const model = (id: string) => models.find((candidate) => candidate.id === id)!;

describe("supports", () => {
  it("matches a language the model lists", () => {
    expect(supports(model("small.en"), "en")).toBe(true);
    expect(supports(model("small.en"), "fr")).toBe(false);
    expect(supports(model("small"), "fr")).toBe(true);
    expect(supports(model("small"), "xx")).toBe(false);
  });
  it("accepts Auto-detect only for a model with more than one language", () => {
    expect(supports(model("small.en"), "auto")).toBe(false);
    expect(supports(model("small"), "auto")).toBe(true);
  });
});
describe("recognizersFor", () => {
  it("keeps the catalog order of the recognition models that support the language", () => {
    expect(recognizersFor(models, "fr").map((m) => m.id)).toEqual([
      "tiny",
      "base",
      "small",
      "large-v3-turbo",
    ]);
    expect(recognizersFor(models, "en")).toHaveLength(7);
  });
});
describe("languageMenu", () => {
  it("lists English first and the rest by name", () => {
    expect(languageMenu(LANGUAGES).map((l) => l.name)).toEqual([
      "English",
      "Cantonese",
      "Chinese",
      "French",
      "German",
    ]);
    expect(languageMenu([...LANGUAGES].reverse())[0].code).toBe("en");
  });
});
describe("modelForLanguage", () => {
  it("keeps a model that supports the language", () => {
    expect(modelForLanguage(models, "en", "base")).toBe("base");
    expect(modelForLanguage(models, "fr", "large-v3-turbo")).toBe(
      "large-v3-turbo",
    );
  });
  it("switches to Small for another language and to Small (English) for English", () => {
    expect(modelForLanguage(models, "fr", "tiny.en")).toBe("small");
    const lacking = catalog();
    lacking.find((m) => m.id === "tiny")!.languages = ["fr"];
    expect(modelForLanguage(lacking, "en", "tiny")).toBe("small.en");
  });
  it("switches to Large Turbo for Cantonese because the other multilingual models lack it", () => {
    expect(supports(model("small"), "yue")).toBe(false);
    expect(modelForLanguage(models, "yue", "small")).toBe("large-v3-turbo");
  });
  it("falls back to the first model that supports the language when Small does not", () => {
    const only = catalog().filter((m) => m.id !== "small");
    expect(modelForLanguage(only, "fr", "tiny.en")).toBe("tiny");
  });
});
