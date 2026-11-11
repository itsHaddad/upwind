// The place search box and its suggestions.

import { type Place, searchPlaces } from "./places";

const DEBOUNCE_MS = 220;

export class SearchBox {
  private readonly input: HTMLInputElement;
  private readonly list: HTMLElement;
  private places: Place[] = [];
  /** The text the listed places were found for. */
  private listedFor = "";
  private selected = 0;
  private pending: AbortController | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    form: HTMLFormElement,
    private readonly onPick: (place: Place) => void,
    private readonly onError: (message: string) => void,
  ) {
    this.input = form.querySelector("input") as HTMLInputElement;
    this.list = form.querySelector("[role=listbox]") as HTMLElement;
    this.input.addEventListener("input", () => this.changed());
    this.input.addEventListener("keydown", (event) => this.key(event));
    this.input.addEventListener("focus", () => this.input.select());
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      void this.submit();
    });
    document.addEventListener("pointerdown", (event) => {
      if (!form.contains(event.target as Node)) this.close();
    });
  }

  /** Shows a place's name in the box without searching. */
  show(place: Place): void {
    this.input.value = place.name;
    this.close();
  }

  private changed(): void {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.search(), DEBOUNCE_MS);
  }

  private async search(): Promise<Place[]> {
    this.pending?.abort();
    const pending = new AbortController();
    this.pending = pending;
    try {
      const query = this.input.value.trim();
      const places = await searchPlaces(query, pending.signal);
      if (pending.signal.aborted) return [];
      this.places = places;
      this.listedFor = query;
      this.selected = 0;
      this.render();
      return places;
    } catch (error) {
      if (!pending.signal.aborted) this.onError(error instanceof Error ? error.message : String(error));
      return [];
    }
  }

  private async submit(): Promise<void> {
    clearTimeout(this.timer);
    // The list may still show places for what was typed a moment ago.
    const upToDate = !this.list.hidden && this.listedFor === this.input.value.trim();
    const place = (upToDate ? this.places[this.selected] : undefined) ?? (await this.search())[0];
    if (place) this.pick(place);
    else if (this.input.value.trim()) this.onError(`No place called "${this.input.value.trim()}" was found.`);
  }

  private key(event: KeyboardEvent): void {
    if (event.key === "Escape") this.close();
    if (this.list.hidden || this.places.length === 0) return;
    const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    this.selected = (this.selected + step + this.places.length) % this.places.length;
    this.render();
  }

  private pick(place: Place): void {
    this.show(place);
    this.input.blur();
    this.onPick(place);
  }

  private close(): void {
    this.list.hidden = true;
  }

  private render(): void {
    this.list.replaceChildren(
      ...this.places.map((place, index) => {
        const item = document.createElement("div");
        item.role = "option";
        item.ariaSelected = String(index === this.selected);
        item.textContent = place.name;
        if (place.detail) {
          const detail = document.createElement("small");
          detail.textContent = place.detail;
          item.append(detail);
        }
        item.addEventListener("pointerdown", (event) => {
          event.preventDefault();
          this.pick(place);
        });
        return item;
      }),
    );
    this.list.hidden = this.places.length === 0;
  }
}
