import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { InstalledModel } from "../types/domain";
import { ModelInfoButton, ModelPicker } from "./ModelPicker";

const models: InstalledModel[] = [
  {
    id: "ggml-large-v3-turbo-bin",
    engine: "whisper.cpp",
    modelName: "ggml-large-v3-turbo.bin",
    variant: "accurate",
    localPath: "/models/ggml-large-v3-turbo.bin",
    sizeBytes: 1_624_555_275,
    isDefault: true,
    profile: "accurate",
  },
  {
    id: "qwen3-asr-1.7b-bf16",
    engine: "qwen3_asr_c",
    modelName: "Qwen3-ASR-1.7B",
    variant: "1.7B BF16",
    localPath: "/models/qwen3-asr-1.7b-bf16",
    sizeBytes: 4_703_041_355,
    isDefault: false,
    profile: "accurate",
  },
];

describe("ModelPicker", () => {
  it("shows a two-line selected value and context-specific recommendation", () => {
    render(
      <ModelPicker
        label="Shortcut Dictation model"
        value={models[0].id}
        models={models}
        context="shortcut_dictation"
        onChange={vi.fn()}
      />,
    );
    const trigger = screen.getByRole("button", { name: "Shortcut Dictation model" });
    expect(within(trigger).getByText("Whisper Turbo")).toBeTruthy();
    expect(within(trigger).getByLabelText(/Speed .* Accuracy/)).toBeTruthy();

    fireEvent.click(trigger);
    const listbox = screen.getByRole("listbox", { name: "Shortcut Dictation model" });
    expect(within(listbox).getByText("Recommended")).toBeTruthy();
    expect(within(listbox).getAllByRole("option")).toHaveLength(2);
  });

  it("selects with mouse and supports arrow and Escape navigation", async () => {
    const onChange = vi.fn();
    render(
      <ModelPicker
        label="Quick Dictate model"
        value={models[0].id}
        models={models}
        context="quick_dictate"
        onChange={onChange}
      />,
    );
    const trigger = screen.getByRole("button", { name: "Quick Dictate model" });
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    const options = screen.getAllByRole("option");
    await waitFor(() => expect(document.activeElement).toBe(options[0]));
    fireEvent.keyDown(options[0], { key: "ArrowDown" });
    expect(document.activeElement).toBe(options[1]);
    fireEvent.click(options[1]);
    expect(onChange).toHaveBeenCalledWith(models[1].id);
    await waitFor(() => expect(document.activeElement).toBe(trigger));

    fireEvent.click(trigger);
    fireEvent.keyDown(screen.getByRole("listbox"), { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("opens model information without selecting and restores focus", async () => {
    const onChange = vi.fn();
    render(
      <ModelPicker
        label="File Transcription model"
        value={models[0].id}
        models={models}
        context="file_transcription"
        onChange={onChange}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "File Transcription model" }));
    const infoButton = screen.getByRole("button", { name: "About Qwen ASR" });
    fireEvent.click(infoButton);
    expect(onChange).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog", { name: "Qwen ASR" });
    expect(within(dialog).getByText("Qwen3-ASR-1.7B")).toBeTruthy();
    expect(within(dialog).getByText("4.7 GB")).toBeTruthy();
    expect(within(dialog).getByText("Recommended for Quick Dictate and File Transcription")).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => {
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(document.activeElement).toBe(infoButton);
    });
  });

  it("shows a disabled empty state", () => {
    render(
      <ModelPicker
        label="Shortcut Dictation model"
        value={null}
        models={[]}
        context="shortcut_dictation"
        onChange={vi.fn()}
      />,
    );
    const trigger = screen.getByRole("button", { name: "Shortcut Dictation model" });
    expect((trigger as HTMLButtonElement).disabled).toBe(true);
    expect(within(trigger).getByText("No models installed")).toBeTruthy();
  });
});

describe("ModelInfoButton", () => {
  it("opens from a download card and closes on backdrop click", () => {
    render(<ModelInfoButton model={models[0]} />);
    const infoButton = screen.getByRole("button", { name: "About Whisper Turbo" });
    fireEvent.click(infoButton);
    const dialog = screen.getByRole("dialog", { name: "Whisper Turbo" });
    fireEvent.mouseDown(dialog.parentElement as HTMLElement);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
