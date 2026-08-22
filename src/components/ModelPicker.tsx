import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import {
  formatModelSize,
  formatRating,
  formatRatingLine,
  getModelPresentation,
  isModelRecommended,
  recommendationLabel,
  type ModelPickerContext,
  type ModelPresentation,
  type PresentableModel,
} from "../lib/modelPresentation";
import type { InstalledModel } from "../types/domain";
import { IconButton } from "./IconButton";

interface ModelPickerProps {
  label: string;
  value: string | null;
  models: InstalledModel[];
  context: ModelPickerContext;
  disabled?: boolean;
  onChange: (modelId: string) => void | Promise<void>;
}

export function ModelPicker({
  label,
  value,
  models,
  context,
  disabled = false,
  onChange,
}: ModelPickerProps) {
  const labelId = useId();
  const panelId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const choiceRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [isOpen, setIsOpen] = useState(false);
  const [infoModel, setInfoModel] = useState<InstalledModel | null>(null);
  const eligibleModels = useMemo(
    () => models.filter((model) => !model.capabilities || model.capabilities.supportedContexts.includes(context)),
    [models, context],
  );
  const presentations = useMemo(() => eligibleModels.map(getModelPresentation), [eligibleModels]);
  const selectedIndex = Math.max(0, eligibleModels.findIndex((model) => model.id === value));
  const selectedModel = eligibleModels.find((model) => model.id === value) ?? eligibleModels[0] ?? null;
  const selectedPresentation = selectedModel ? getModelPresentation(selectedModel) : null;

  useEffect(() => {
    if (!isOpen) return;
    const closeOnOutsideClick = (event: MouseEvent) => {
      if (event.target instanceof Node && !rootRef.current?.contains(event.target)) {
        setIsOpen(false);
      }
    };
    document.addEventListener("mousedown", closeOnOutsideClick);
    return () => document.removeEventListener("mousedown", closeOnOutsideClick);
  }, [isOpen]);

  function closeAndRestoreFocus() {
    setIsOpen(false);
    window.requestAnimationFrame(() => triggerRef.current?.focus());
  }

  function handleTriggerKeyDown(event: ReactKeyboardEvent<HTMLButtonElement>) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp" || event.key === "Home" || event.key === "End") {
      event.preventDefault();
      setIsOpen(true);
      const nextIndex = event.key === "End" ? eligibleModels.length - 1 : selectedIndex;
      window.requestAnimationFrame(() => choiceRefs.current[nextIndex]?.focus());
    }
  }

  function handlePanelKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      closeAndRestoreFocus();
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const choices = choiceRefs.current.filter(Boolean) as HTMLButtonElement[];
    if (choices.length === 0) return;
    event.preventDefault();
    const currentIndex = choices.indexOf(document.activeElement as HTMLButtonElement);
    const nextIndex =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? choices.length - 1
          : currentIndex < 0
            ? selectedIndex
            : (currentIndex + (event.key === "ArrowDown" ? 1 : -1) + choices.length) % choices.length;
    choices[nextIndex]?.focus();
  }

  return (
    <div className="model-picker" ref={rootRef}>
      <span id={labelId} className="model-picker-label">{label}</span>
      <button
        ref={triggerRef}
        type="button"
        className="model-picker-trigger"
        aria-labelledby={labelId}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        aria-controls={isOpen ? panelId : undefined}
        disabled={disabled || eligibleModels.length === 0}
        onClick={() => {
          setIsOpen((current) => !current);
          if (!isOpen) {
            window.requestAnimationFrame(() => choiceRefs.current[selectedIndex]?.focus());
          }
        }}
        onKeyDown={handleTriggerKeyDown}
      >
        {selectedPresentation ? (
          <ModelSummary presentation={selectedPresentation} />
        ) : (
          <span className="model-picker-empty">No models installed</span>
        )}
        <span className={isOpen ? "model-picker-chevron is-open" : "model-picker-chevron"} aria-hidden="true">⌄</span>
      </button>

      {isOpen ? (
        <div
          id={panelId}
          className="model-picker-panel"
          role="listbox"
          aria-labelledby={labelId}
          onKeyDown={handlePanelKeyDown}
        >
          {eligibleModels.map((model, index) => {
            const presentation = presentations[index];
            const isSelected = model.id === selectedModel?.id;
            return (
              <div key={model.id} className={isSelected ? "model-picker-option-row is-selected" : "model-picker-option-row"}>
                <button
                  ref={(node) => { choiceRefs.current[index] = node; }}
                  type="button"
                  className="model-picker-option"
                  role="option"
                  aria-selected={isSelected}
                  tabIndex={isSelected ? 0 : -1}
                  onClick={() => {
                    void onChange(model.id);
                    closeAndRestoreFocus();
                  }}
                >
                  <ModelSummary
                    presentation={presentation}
                    recommendation={isModelRecommended(presentation, context) ? "Recommended" : null}
                  />
                </button>
                <IconButton
                  icon="info"
                  label={`About ${presentation.friendlyName}`}
                  size="compact"
                  className="model-info-button"
                  onClick={(event) => {
                    event.currentTarget.focus();
                    setInfoModel(model);
                  }}
                />
              </div>
            );
          })}
        </div>
      ) : null}

      {infoModel ? (
        <ModelInfoDialog model={infoModel} onClose={() => setInfoModel(null)} />
      ) : null}
    </div>
  );
}

export function ModelSummary({
  presentation,
  recommendation = null,
}: {
  presentation: ModelPresentation;
  recommendation?: string | null;
}) {
  return (
    <span className="model-summary">
      <span className="model-summary-name-row">
        <strong>{presentation.friendlyName}</strong>
        {recommendation ? <span className="model-recommendation">{recommendation}</span> : null}
      </span>
      <span className="model-rating-line" aria-label={formatRatingLine(presentation)}>
        <span>Speed <span aria-hidden="true">{formatRating(presentation.speed)}</span></span>
        <span>Accuracy <span aria-hidden="true">{formatRating(presentation.accuracy)}</span></span>
      </span>
    </span>
  );
}

export function ModelInfoButton({ model }: { model: PresentableModel }) {
  const [isOpen, setIsOpen] = useState(false);
  return (
    <>
      <IconButton
        icon="info"
        label={`About ${getModelPresentation(model).friendlyName}`}
        size="compact"
        className="model-info-button"
        onClick={(event) => {
          event.currentTarget.focus();
          setIsOpen(true);
        }}
      />
      {isOpen ? <ModelInfoDialog model={model} onClose={() => setIsOpen(false)} /> : null}
    </>
  );
}

export function ModelInfoDialog({
  model,
  onClose,
}: {
  model: PresentableModel;
  onClose: () => void;
}) {
  const titleId = useId();
  const closeRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const invokerRef = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);
  const presentation = getModelPresentation(model);

  useEffect(() => {
    closeRef.current?.focus();
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("keydown", closeOnEscape);
      window.requestAnimationFrame(() => invokerRef.current?.focus());
    };
  }, [onClose]);

  function trapFocus(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (event.key !== "Tab") return;
    const focusable = Array.from(
      event.currentTarget.querySelectorAll<HTMLElement>('button:not([disabled]), [href], input:not([disabled]), [tabindex]:not([tabindex="-1"])'),
    );
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return createPortal(
    <div
      className="model-info-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        ref={dialogRef}
        className="model-info-dialog glass-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onKeyDown={trapFocus}
      >
        <div className="model-info-header">
          <div>
            <h2 id={titleId}>{presentation.friendlyName}</h2>
            <p className="model-technical-name">{presentation.technicalName}</p>
          </div>
          <IconButton ref={closeRef} icon="xmark" label="Close model information" size="compact" onClick={onClose} />
        </div>
        <dl className="model-info-facts">
          <div><dt>Size</dt><dd>{formatModelSize(presentation.sizeBytes)}</dd></div>
          <div><dt>Speed</dt><dd aria-label={`Speed ${presentation.speed} of 5`}>{formatRating(presentation.speed)}</dd></div>
          <div><dt>Accuracy</dt><dd aria-label={`Accuracy ${presentation.accuracy} of 5`}>{formatRating(presentation.accuracy)}</dd></div>
        </dl>
        <p className="model-info-description">{presentation.description}</p>
        <div className="model-info-technical">
          <strong>Technical details</strong>
          <p>{presentation.technicalDetails}</p>
          {presentation.requirements ? <p>{presentation.requirements}</p> : null}
          {model.capabilities?.nativeDiarization ? <p>Built-in speaker identification and timestamps · standalone speaker processing is skipped</p> : null}
          {model.capabilities?.contextSupport ? <p>Uses your Blabber vocabulary as model context or hotwords</p> : null}
          {model.capabilities?.languageControl === "automatic_only" ? <p>Automatic language detection and code-switching · the global fixed-language choice is not applied</p> : null}
        </div>
        {presentation.recommendedFor.length > 0 ? (
          <p className="model-info-recommended">
            Recommended for {presentation.recommendedFor.map(recommendationLabel).join(" and ")}
          </p>
        ) : null}
      </div>
    </div>,
    document.body,
  );
}
