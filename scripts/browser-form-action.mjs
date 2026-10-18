// Embedded by Rust and evaluated on the authorized page in either browser backend.
/* global document, HTMLInputElement, HTMLTextAreaElement, HTMLSelectElement, HTMLFormElement, DataTransfer, File, atob, InputEvent, Event */
export function formAction({ action, selector, text, values, checked, files }) {
  const matches = document.querySelectorAll(selector);
  if (matches.length !== 1) throw new Error(`Expected one field for ${selector}; found ${matches.length}`);
  const element = matches[0];
  const fireChange = () => {
    element.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertReplacementText" }));
    element.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const setNative = (prototype, property, value) => {
    Object.getOwnPropertyDescriptor(prototype, property).set.call(element, value);
  };
  if (action === "validate") {
    if (!(element instanceof HTMLFormElement)) throw new Error("validate requires a form selector");
    return {
      valid: element.checkValidity(),
      invalid: [...element.elements].filter(field => field.willValidate && !field.validity.valid)
        .map(field => ({ id: field.id, name: field.name, validation_message: field.validationMessage })),
    };
  }
  if (element.matches(":disabled")) throw new Error("The field is disabled");
  if (element.readOnly) throw new Error("The field is read-only");
  element.scrollIntoView({ block: "center", inline: "nearest" });
  element.focus();
  if (action === "fill") {
    if (typeof text !== "string") throw new Error("fill requires text");
    if (element instanceof HTMLTextAreaElement) setNative(HTMLTextAreaElement.prototype, "value", text);
    else if (element instanceof HTMLInputElement && !["file", "checkbox", "radio", "button", "submit", "reset", "image", "hidden"].includes(element.type)) {
      setNative(HTMLInputElement.prototype, "value", text);
    } else if (element.isContentEditable) element.textContent = text;
    else throw new Error("The target is not an editable text/date field");
    fireChange();
    const actual = element.isContentEditable ? element.textContent : element.value;
    if (actual !== text) throw new Error(`The page rejected the requested value; observed ${JSON.stringify(actual)}`);
    return { value: element.type === "password" ? "[redacted]" : actual, valid: element.validity?.valid ?? null, validation_message: element.validationMessage ?? null };
  }
  if (action === "select") {
    if (!(element instanceof HTMLSelectElement)) throw new Error("select requires a select element");
    if (!Array.isArray(values) || values.some(value => typeof value !== "string")) throw new Error("select requires values[]");
    if (new Set(values).size !== values.length) throw new Error("select values must be unique");
    if (!element.multiple && values.length > 1) throw new Error("This field does not allow multiple values");
    for (const value of values) {
      const option = [...element.options].find(option => option.value === value);
      if (!option || option.disabled || option.parentElement?.disabled) throw new Error(`Option ${JSON.stringify(value)} is unavailable`);
    }
    for (const option of element.options) option.selected = values.includes(option.value);
    if (!values.length) element.selectedIndex = -1;
    fireChange();
    const actual = [...element.selectedOptions].map(option => option.value);
    if (actual.length !== values.length || actual.some(value => !values.includes(value))) throw new Error(`The page rejected the selection; observed ${JSON.stringify(actual)}`);
    return { values: actual };
  }
  if (action === "check") {
    if (!(element instanceof HTMLInputElement) || !["checkbox", "radio"].includes(element.type)) throw new Error("check requires a checkbox or radio");
    if (typeof checked !== "boolean") throw new Error("check requires checked");
    if (element.type === "radio" && !checked && element.checked) throw new Error("A radio must be changed by selecting another option in its group");
    if (element.checked !== checked) element.click();
    if (element.checked !== checked) throw new Error(`The page rejected the checked state; observed ${element.checked}`);
    return { checked: element.checked };
  }
  if (action === "upload") {
    if (!(element instanceof HTMLInputElement) || element.type !== "file") throw new Error("upload requires a file input");
    if (!Array.isArray(files)) throw new Error("upload requires authorized files");
    if (!element.multiple && files.length > 1) throw new Error("This field allows only one file");
    const transfer = new DataTransfer();
    for (const file of files) {
      const bytes = Uint8Array.from(atob(file.data), character => character.charCodeAt(0));
      transfer.items.add(new File([bytes], file.name, { type: file.mime_type }));
    }
    setNative(HTMLInputElement.prototype, "files", transfer.files);
    fireChange();
    const actual = [...element.files].map(file => ({ name: file.name, bytes: file.size, mime_type: file.type }));
    if (actual.length !== files.length) throw new Error("The page rejected the uploaded file selection");
    return { files: actual };
  }
  throw new Error(`Unknown form action ${action}`);
}
