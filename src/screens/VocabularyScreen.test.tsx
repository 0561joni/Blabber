import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { VocabularyScreen } from "./VocabularyScreen";
import type { VocabularyTerm } from "../types/domain";

const term: VocabularyTerm = {
  id: "term-1",
  canonical: "Blabber",
  normalizedCanonical: "blabber",
  category: "brand",
  languageHint: "en",
  matchMode: "exact_and_fuzzy",
  isBuiltin: false,
  createdAt: "2026-08-17T00:00:00Z",
  updatedAt: "2026-08-17T00:00:00Z",
  aliases: [],
};

describe("VocabularyScreen symbolic actions", () => {
  it("uses named icon actions for add, edit, save, cancel, and delete", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    const onUpdate = vi.fn().mockResolvedValue(undefined);
    const onDelete = vi.fn().mockResolvedValue(undefined);
    render(
      <VocabularyScreen
        vocabularyTerms={[term]}
        onCreateVocabularyTerm={onCreate}
        onUpdateVocabularyTerm={onUpdate}
        onDeleteVocabularyTerm={onDelete}
      />,
    );

    expect(screen.getByRole("button", { name: "Add vocabulary term" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Edit Blabber" }));
    expect(screen.getByRole("button", { name: "Save Blabber" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Cancel editing Blabber" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Cancel editing Blabber" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete Blabber" }));
    await waitFor(() => expect(onDelete).toHaveBeenCalledWith("term-1"));
  });
});
