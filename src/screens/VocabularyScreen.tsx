import { useState } from "react";
import { IconButton } from "../components/IconButton";
import type {
  CreateVocabularyTermInput,
  UpdateVocabularyTermInput,
  VocabularyMatchMode,
  VocabularyTerm,
} from "../types/domain";

interface VocabularyScreenProps {
  vocabularyTerms: VocabularyTerm[];
  onCreateVocabularyTerm: (input: CreateVocabularyTermInput) => Promise<void>;
  onUpdateVocabularyTerm: (termId: string, input: UpdateVocabularyTermInput) => Promise<void>;
  onDeleteVocabularyTerm: (termId: string) => Promise<void>;
}

type EditorDraft = {
  canonical: string;
  aliases: string;
  category: string;
  languageHint: string;
  matchMode: VocabularyMatchMode;
};

const EMPTY_EDITOR: EditorDraft = {
  canonical: "",
  aliases: "",
  category: "",
  languageHint: "",
  matchMode: "exact_and_fuzzy",
};

export function VocabularyScreen({
  vocabularyTerms,
  onCreateVocabularyTerm,
  onUpdateVocabularyTerm,
  onDeleteVocabularyTerm,
}: VocabularyScreenProps) {
  const [isSaving, setIsSaving] = useState(false);
  const [createDraft, setCreateDraft] = useState(EMPTY_EDITOR);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editDraft, setEditDraft] = useState(EMPTY_EDITOR);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  async function createTerm() {
    setIsSaving(true);
    setErrorMessage(null);
    try {
      await onCreateVocabularyTerm(normalizeEditorInput(createDraft));
      setCreateDraft(EMPTY_EDITOR);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : "Failed to create vocabulary term.");
    } finally {
      setIsSaving(false);
    }
  }

  function beginEdit(term: VocabularyTerm) {
    setEditingId(term.id);
    setEditDraft({
      canonical: term.canonical,
      aliases: term.aliases.map((alias) => alias.alias).join(", "),
      category: term.category,
      languageHint: term.languageHint ?? "",
      matchMode: term.matchMode,
    });
  }

  async function saveEdit(termId: string) {
    setIsSaving(true);
    setErrorMessage(null);
    try {
      await onUpdateVocabularyTerm(termId, normalizeEditorInput(editDraft));
      setEditingId(null);
      setEditDraft(EMPTY_EDITOR);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : "Failed to update vocabulary term.");
    } finally {
      setIsSaving(false);
    }
  }

  async function deleteTerm(termId: string) {
    setIsSaving(true);
    setErrorMessage(null);
    try {
      await onDeleteVocabularyTerm(termId);
      if (editingId === termId) {
        setEditingId(null);
        setEditDraft(EMPTY_EDITOR);
      }
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : "Failed to delete vocabulary term.");
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <section className="screen">
      <div className="glass-panel vocabulary-card">
        <div className="section-header">
          <div>
            <p className="eyebrow">Offline correction</p>
            <h2>Custom vocabulary</h2>
            <p className="muted">
              Terms here are applied before insertion and saved transcript output.
            </p>
          </div>
        </div>

        {errorMessage ? <p className="error-text">{errorMessage}</p> : null}

        <div className="vocabulary-editor">
          <label className="field-stack">
            <span>Canonical term</span>
            <input
              placeholder="LinkedIn"
              value={createDraft.canonical}
              onChange={(event) =>
                setCreateDraft((current) => ({ ...current, canonical: event.target.value }))
              }
            />
          </label>
          <label className="field-stack">
            <span>Aliases</span>
            <input
              placeholder="linked in, linken"
              value={createDraft.aliases}
              onChange={(event) =>
                setCreateDraft((current) => ({ ...current, aliases: event.target.value }))
              }
            />
          </label>
          <label className="field-stack">
            <span>Category</span>
            <input
              placeholder="brand"
              value={createDraft.category}
              onChange={(event) =>
                setCreateDraft((current) => ({ ...current, category: event.target.value }))
              }
            />
          </label>
          <label className="field-stack">
            <span>Language hint</span>
            <input
              placeholder="en"
              value={createDraft.languageHint}
              onChange={(event) =>
                setCreateDraft((current) => ({ ...current, languageHint: event.target.value }))
              }
            />
          </label>
          <label className="field-stack">
            <span>Matching</span>
            <select
              value={createDraft.matchMode}
              onChange={(event) =>
                setCreateDraft((current) => ({
                  ...current,
                  matchMode: event.target.value as "exact_only" | "exact_and_fuzzy",
                }))
              }
            >
              <option value="exact_only">Strict match only</option>
              <option value="exact_and_fuzzy">Allow close matches</option>
            </select>
          </label>
          <IconButton
            icon="plus"
            label="Add vocabulary term"
            disabled={isSaving || createDraft.canonical.trim().length === 0}
            onClick={() => void createTerm()}
          />
        </div>

        <div className="vocabulary-list">
          {vocabularyTerms.map((term) => {
            const isEditing = editingId === term.id;
            const draft = isEditing
              ? editDraft
              : {
                  canonical: term.canonical,
                  aliases: term.aliases.map((alias) => alias.alias).join(", "),
                  category: term.category,
                  languageHint: term.languageHint ?? "",
                  matchMode: term.matchMode,
                };

            return (
              <div className="vocabulary-row" key={term.id}>
                <div className="vocabulary-row-header">
                  <div>
                    <p className="transcript-title">{term.canonical}</p>
                    <p className="muted">
                      {term.category}
                      {term.languageHint ? ` · ${term.languageHint}` : ""}
                    </p>
                  </div>
                  <span className="language-chip">{term.isBuiltin ? "built-in" : "custom"}</span>
                </div>

                {isEditing ? (
                  <div className="vocabulary-editor compact">
                    <label className="field-stack">
                      <span>Canonical term</span>
                      <input
                        value={draft.canonical}
                        onChange={(event) =>
                          setEditDraft((current) => ({ ...current, canonical: event.target.value }))
                        }
                      />
                    </label>
                    <label className="field-stack">
                      <span>Aliases</span>
                      <input
                        value={draft.aliases}
                        onChange={(event) =>
                          setEditDraft((current) => ({ ...current, aliases: event.target.value }))
                        }
                      />
                    </label>
                    <label className="field-stack">
                      <span>Category</span>
                      <input
                        value={draft.category}
                        onChange={(event) =>
                          setEditDraft((current) => ({ ...current, category: event.target.value }))
                        }
                      />
                    </label>
                    <label className="field-stack">
                      <span>Language hint</span>
                      <input
                        value={draft.languageHint}
                        onChange={(event) =>
                          setEditDraft((current) => ({ ...current, languageHint: event.target.value }))
                        }
                      />
                    </label>
                    <label className="field-stack">
                      <span>Matching</span>
                      <select
                        value={draft.matchMode}
                        onChange={(event) =>
                          setEditDraft((current) => ({
                            ...current,
                            matchMode: event.target.value as "exact_only" | "exact_and_fuzzy",
                          }))
                        }
                      >
                        <option value="exact_only">Strict match only</option>
                        <option value="exact_and_fuzzy">Allow close matches</option>
                      </select>
                    </label>
                    <div className="toolbar action-segment">
                      <IconButton
                        icon="check"
                        label={`Save ${term.canonical}`}
                        state="success"
                        disabled={isSaving || draft.canonical.trim().length === 0}
                        onClick={() => void saveEdit(term.id)}
                      />
                      <IconButton
                        icon="xmark"
                        label={`Cancel editing ${term.canonical}`}
                        disabled={isSaving}
                        onClick={() => {
                          setEditingId(null);
                          setEditDraft(EMPTY_EDITOR);
                        }}
                      />
                    </div>
                  </div>
                ) : (
                  <div className="vocabulary-row-footer">
                    <p className="muted">
                      Aliases: {term.aliases.length > 0 ? term.aliases.map((alias) => alias.alias).join(", ") : "None"}
                    </p>
                    <p className="muted">
                      Matching: {term.matchMode === "exact_only" ? "Strict match only" : "Allow close matches"}
                    </p>
                    <div className="toolbar action-segment">
                      {!term.isBuiltin ? (
                        <>
                          <IconButton icon="pencil" label={`Edit ${term.canonical}`} disabled={isSaving} onClick={() => beginEdit(term)} />
                          <IconButton icon="trash" label={`Delete ${term.canonical}`} tone="danger" disabled={isSaving} onClick={() => void deleteTerm(term.id)} />
                        </>
                      ) : null}
                    </div>
                  </div>
                )}
              </div>
            );
          })}

          {vocabularyTerms.length === 0 ? (
            <p className="muted">No vocabulary terms yet.</p>
          ) : null}
        </div>
      </div>
    </section>
  );
}

function normalizeEditorInput(input: {
  canonical: string;
  aliases: string;
  category: string;
  languageHint: string;
  matchMode: VocabularyMatchMode;
}): CreateVocabularyTermInput {
  return {
    canonical: input.canonical.trim(),
    aliases: input.aliases
      .split(",")
      .map((alias) => alias.trim())
      .filter((alias) => alias.length > 0),
    category: input.category.trim() || null,
    languageHint: input.languageHint.trim() || null,
    matchMode: input.matchMode,
  };
}
