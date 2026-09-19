import type { ChangeEvent } from "react";

type SchemaProperty = {
  type?: string;
  title?: string;
  description?: string;
  enum?: unknown[];
  enumNames?: string[];
  format?: string;
  secret?: boolean;
};

type EditorKind =
  | "string"
  | "secret-string"
  | "boolean"
  | "integer"
  | "number"
  | "enum"
  | "textarea"
  | "unsupported";

export interface SchemaFormProps {
  schema: unknown;
  value: Record<string, unknown>;
  onChange: (value: Record<string, unknown>) => void;
  secretValues: Record<string, string>;
  onSecretValueChange: (name: string, value: string) => void;
  secretConfigured: Record<string, boolean>;
  idPrefix: string;
  disabled?: boolean;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readProperties(schema: unknown): Record<string, SchemaProperty> {
  if (!isRecord(schema) || !isRecord(schema.properties)) {
    return {};
  }

  return Object.fromEntries(
    Object.entries(schema.properties).filter(
      (entry): entry is [string, SchemaProperty] => isRecord(entry[1]),
    ),
  );
}

function readRequired(schema: unknown): Set<string> {
  if (!isRecord(schema) || !Array.isArray(schema.required)) {
    return new Set();
  }

  return new Set(
    schema.required.filter((item): item is string => typeof item === "string"),
  );
}

function editorKind(property: SchemaProperty): EditorKind {
  if (
    property.type === "secret-string" ||
    property.format === "password" ||
    property.secret === true
  ) {
    return "secret-string";
  }
  if (property.type === "textarea" || property.format === "textarea") {
    return "textarea";
  }
  if (Array.isArray(property.enum) || property.type === "enum") {
    return "enum";
  }
  if (
    property.type === "string" ||
    property.type === "boolean" ||
    property.type === "integer" ||
    property.type === "number"
  ) {
    return property.type;
  }
  return "unsupported";
}

export function getSecretFieldNames(schema: unknown): string[] {
  return Object.entries(readProperties(schema))
    .filter(([, property]) => editorKind(property) === "secret-string")
    .map(([name]) => name);
}

function fieldLabel(name: string, property: SchemaProperty): string {
  return property.title?.trim() || name;
}

function FieldDescription({ id, text }: { id: string; text?: string }) {
  return text ? (
    <span className="schema-field-description" id={id}>
      {text}
    </span>
  ) : null;
}

export function SchemaForm({
  schema,
  value,
  onChange,
  secretValues,
  onSecretValueChange,
  secretConfigured,
  idPrefix,
  disabled = false,
}: SchemaFormProps) {
  const properties = readProperties(schema);
  const required = readRequired(schema);

  const setField = (name: string, fieldValue: unknown) => {
    onChange({ ...value, [name]: fieldValue });
  };

  const renderField = (name: string, property: SchemaProperty, index: number) => {
    const kind = editorKind(property);
    const label = fieldLabel(name, property);
    const inputId = `${idPrefix}-field-${index}`;
    const descriptionId = `${inputId}-description`;
    const currentValue = value[name];
    const commonProps = {
      id: inputId,
      name,
      disabled,
      required: required.has(name),
      "aria-describedby": property.description ? descriptionId : undefined,
    };

    if (kind === "unsupported") {
      return (
        <div className="schema-field schema-field--unsupported" key={name}>
          <span className="schema-field-label">{label}</span>
          <p className="schema-field-unsupported">当前版本无法编辑此字段</p>
          <FieldDescription id={descriptionId} text={property.description} />
        </div>
      );
    }

    let control;
    if (kind === "boolean") {
      control = (
        <label className="schema-checkbox" htmlFor={inputId}>
          <input
            {...commonProps}
            type="checkbox"
            checked={currentValue === true}
            onChange={(event) => setField(name, event.currentTarget.checked)}
          />
          <span>{label}</span>
        </label>
      );
    } else if (kind === "enum") {
      const options = property.enum ?? [];
      const selectedIndex = options.findIndex((option) => option === currentValue);
      control = (
        <>
          <label className="schema-field-label" htmlFor={inputId}>
            {label}
            {required.has(name) ? <span aria-hidden="true"> *</span> : null}
          </label>
          <select
            {...commonProps}
            value={selectedIndex >= 0 ? String(selectedIndex) : ""}
            onChange={(event) => {
              const index = Number.parseInt(event.currentTarget.value, 10);
              setField(name, options[index]);
            }}
          >
            <option value="">请选择</option>
            {options.map((option, optionIndex) => (
              <option value={String(optionIndex)} key={`${name}-${optionIndex}`}>
                {property.enumNames?.[optionIndex] ?? String(option)}
              </option>
            ))}
          </select>
        </>
      );
    } else if (kind === "textarea") {
      control = (
        <>
          <label className="schema-field-label" htmlFor={inputId}>
            {label}
            {required.has(name) ? <span aria-hidden="true"> *</span> : null}
          </label>
          <textarea
            {...commonProps}
            rows={3}
            value={typeof currentValue === "string" ? currentValue : ""}
            onChange={(event) => setField(name, event.currentTarget.value)}
          />
        </>
      );
    } else if (kind === "secret-string") {
      control = (
        <>
          <div className="schema-secret-label">
            <label className="schema-field-label" htmlFor={inputId}>
              {label}
              {required.has(name) ? <span aria-hidden="true"> *</span> : null}
            </label>
            <span className="schema-secret-status">
              {secretConfigured[name] ? "已配置" : "未配置"}
            </span>
          </div>
          <input
            {...commonProps}
            type="password"
            autoComplete="new-password"
            value={secretValues[name] ?? ""}
            placeholder={secretConfigured[name] ? "留空则保留当前值" : "请输入"}
            onChange={(event: ChangeEvent<HTMLInputElement>) =>
              onSecretValueChange(name, event.currentTarget.value)
            }
          />
        </>
      );
    } else if (kind === "integer" || kind === "number") {
      control = (
        <>
          <label className="schema-field-label" htmlFor={inputId}>
            {label}
            {required.has(name) ? <span aria-hidden="true"> *</span> : null}
          </label>
          <input
            {...commonProps}
            type="number"
            step={kind === "integer" ? 1 : "any"}
            value={typeof currentValue === "number" ? String(currentValue) : ""}
            onChange={(event) => {
              const raw = event.currentTarget.value;
              if (raw === "") {
                setField(name, "");
                return;
              }
              setField(
                name,
                kind === "integer" ? Number.parseInt(raw, 10) : Number(raw),
              );
            }}
          />
        </>
      );
    } else {
      control = (
        <>
          <label className="schema-field-label" htmlFor={inputId}>
            {label}
            {required.has(name) ? <span aria-hidden="true"> *</span> : null}
          </label>
          <input
            {...commonProps}
            type="text"
            value={typeof currentValue === "string" ? currentValue : ""}
            onChange={(event) => setField(name, event.currentTarget.value)}
          />
        </>
      );
    }

    return (
      <div className={`schema-field schema-field--${kind}`} key={name}>
        {control}
        <FieldDescription id={descriptionId} text={property.description} />
      </div>
    );
  };

  const entries = Object.entries(properties);
  if (entries.length === 0) {
    return <p className="schema-field-unsupported">当前版本无法编辑此字段</p>;
  }

  return (
    <div className="schema-form-fields">
      {entries.map(([name, property], index) =>
        renderField(name, property, index),
      )}
    </div>
  );
}
