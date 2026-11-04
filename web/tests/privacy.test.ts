// @vitest-environment jsdom
import { describe, expect, test } from "vitest";
import { maskTextNode, watchMasked } from "../src/shared/privacy.js";

describe("privacy pseudonyms", () => {
  test("are wrapped in blurred spans and other text is kept", () => {
    const cell = document.createElement("td");
    cell.textContent = "ssh ░1a2b3c4d░@░deadbeef░ -p 22";
    maskTextNode(cell.firstChild as Text);
    const masked = [...cell.querySelectorAll(".masked")];
    expect(masked.map((span) => span.textContent)).toEqual([
      "░1a2b3c4d░",
      "░deadbeef░",
    ]);
    expect(cell.textContent).toBe("ssh ░1a2b3c4d░@░deadbeef░ -p 22");
  });

  test("text without pseudonyms is left alone", () => {
    const cell = document.createElement("td");
    cell.textContent = "progress ░░░ 50%";
    const text = cell.firstChild as Text;
    maskTextNode(text);
    expect(cell.firstChild).toBe(text);
  });

  test("text rendered later is masked", async () => {
    const root = document.createElement("div");
    document.body.append(root);
    const observer = watchMasked(root);
    const label = document.createElement("span");
    root.append(label);
    label.textContent = "user ░00ff00ff░";
    await Promise.resolve();
    expect(label.querySelector(".masked")?.textContent).toBe("░00ff00ff░");
    (label.firstChild as Text).data = "host ░12345678░, user ";
    await Promise.resolve();
    expect(label.textContent).toBe("host ░12345678░, user ░00ff00ff░");
    expect(label.querySelectorAll(".masked")).toHaveLength(2);
    observer.disconnect();
  });
});
