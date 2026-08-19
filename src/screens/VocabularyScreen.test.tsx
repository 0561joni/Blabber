import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { VocabularyTerm } from "../types/domain";
import { VocabularyScreen } from "./VocabularyScreen";

const customTerm: VocabularyTerm = {
  id: "term-1",
  canonical: "Blabber",
  normalizedCanonical: "blabber",
  matchMode: "exact_and_fuzzy",
  isBuiltin: false,
  createdAt: "2026-08-17T00:00:00Z",
  updatedAt: "2026-08-17T00:00:00Z",
  aliases: [
    { id: "alias-1", alias: "blaber", normalizedAlias: "blaber" },
  ],
};

const builtinTerm: VocabularyTerm = {
  id: "builtin-linkedin",
  canonical: "LinkedIn",
  normalizedCanonical: "linkedin",
  matchMode: "exact_only",
  isBuiltin: true,
  createdAt: "2026-03-14T00:00:00Z",
  updatedAt: "2026-03-14T00:00:00Z",
  aliases: [
    { id: "alias-2", alias: "linked in", normalizedAlias: "linked in" },
  ],
};

const defaultHandlers = {
  onCreateVocabularyTerm: vi.fn().mockResolvedValue(undefined),
  onUpdateVocabularyTerm: vi.fn().mockResolvedValue(undefined),
  onDeleteVocabularyTerm: vi.fn().mockResolvedValue(undefined),
};

describe("VocabularyScreen", () => {
  it("uses a bounded workspace and omits category and language fields", () => {
    render(
      <VocabularyScreen
        vocabularyTerms={[customTerm, builtinTerm]}
        {...defaultHandlers}
      />,
    );

    expect(document.querySelector(".vocabulary-workspace")).toBeTruthy();
    expect(screen.getByLabelText("Correct spelling")).toBeTruthy();
    expect(screen.getByLabelText("Common mishearings or alternatives")).toBeTruthy();
    expect(screen.queryByText("Category")).toBeNull();
    expect(screen.queryByText("Language hint")).toBeNull();
    expect(screen.getByText("Blabber")).toBeTruthy();
    expect(screen.getByText("blaber")).toBeTruthy();
  });

  it("creates a trimmed term with aliases and close matching enabled by default", async () => {
    const onCreateVocabularyTerm = vi.fn().mockResolvedValue(undefined);
    render(
      <VocabularyScreen
        vocabularyTerms={[]}
        {...defaultHandlers}
        onCreateVocabularyTerm={onCreateVocabularyTerm}
      />,
    );
    const form = screen.getByRole("form", { name: "Add vocabulary term" });
    const matchSwitch = within(form).getByRole("switch", {
      name: "Correct close matches for new term",
    });

    expect(matchSwitch.getAttribute("aria-checked")).toBe("true");
    fireEvent.change(within(form).getByLabelText("Correct spelling"), {
      target: { value: "  CloudOpus  " },
    });
    fireEvent.change(within(form).getByLabelText("Common mishearings or alternatives"), {
      target: { value: " cloud opus, cloud oppus, " },
    });
    fireEvent.click(within(form).getByRole("button", { name: "Add term" }));

    await waitFor(() =>
      expect(onCreateVocabularyTerm).toHaveBeenCalledWith({
        canonical: "CloudOpus",
        aliases: ["cloud opus", "cloud oppus"],
        matchMode: "exact_and_fuzzy",
      }),
    );
  });

  it("maps the close-match switch to exact-only matching", async () => {
    const onCreateVocabularyTerm = vi.fn().mockResolvedValue(undefined);
    render(
      <VocabularyScreen
        vocabularyTerms={[]}
        {...defaultHandlers}
        onCreateVocabularyTerm={onCreateVocabularyTerm}
      />,
    );
    const form = screen.getByRole("form", { name: "Add vocabulary term" });
    fireEvent.change(within(form).getByLabelText("Correct spelling"), {
      target: { value: "Obsidian" },
    });
    fireEvent.click(
      within(form).getByRole("switch", { name: "Correct close matches for new term" }),
    );
    fireEvent.submit(form);

    await waitFor(() =>
      expect(onCreateVocabularyTerm).toHaveBeenCalledWith({
        canonical: "Obsidian",
        aliases: [],
        matchMode: "exact_only",
      }),
    );
  });

  it("edits a custom term inline using the simplified payload", async () => {
    const onUpdateVocabularyTerm = vi.fn().mockResolvedValue(undefined);
    render(
      <VocabularyScreen
        vocabularyTerms={[customTerm]}
        {...defaultHandlers}
        onUpdateVocabularyTerm={onUpdateVocabularyTerm}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Edit Blabber" }));
    const editForm = screen.getByRole("form", { name: "Edit Blabber" });
    fireEvent.change(within(editForm).getByLabelText("Correct spelling"), {
      target: { value: "Blabber App" },
    });
    fireEvent.change(within(editForm).getByLabelText("Common mishearings or alternatives"), {
      target: { value: "blaber, blabber app" },
    });
    fireEvent.click(
      within(editForm).getByRole("switch", { name: "Correct close matches for Blabber" }),
    );
    fireEvent.click(within(editForm).getByRole("button", { name: "Save changes" }));

    await waitFor(() =>
      expect(onUpdateVocabularyTerm).toHaveBeenCalledWith("term-1", {
        canonical: "Blabber App",
        aliases: ["blaber", "blabber app"],
        matchMode: "exact_only",
      }),
    );
  });

  it("keeps built-in terms collapsed and read-only", () => {
    render(
      <VocabularyScreen
        vocabularyTerms={[builtinTerm]}
        {...defaultHandlers}
      />,
    );
    const disclosure = screen.getByRole("button", { name: /Included by default/ });

    expect(disclosure.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("LinkedIn")).toBeNull();
    fireEvent.click(disclosure);
    expect(disclosure.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("LinkedIn")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Edit LinkedIn" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Delete LinkedIn" })).toBeNull();
  });

  it("shows an actionable empty state and retains custom deletion", async () => {
    const onDeleteVocabularyTerm = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(
      <VocabularyScreen
        vocabularyTerms={[]}
        {...defaultHandlers}
        onDeleteVocabularyTerm={onDeleteVocabularyTerm}
      />,
    );
    expect(screen.getByText("No custom terms yet")).toBeTruthy();

    rerender(
      <VocabularyScreen
        vocabularyTerms={[customTerm]}
        {...defaultHandlers}
        onDeleteVocabularyTerm={onDeleteVocabularyTerm}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Delete Blabber" }));
    await waitFor(() => expect(onDeleteVocabularyTerm).toHaveBeenCalledWith("term-1"));
  });
});
