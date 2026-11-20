import { goto } from "$app/navigation";
import { page } from "$app/state";

export function currentCatalogPage(): number {
  const value = Number(page.url.searchParams.get("page"));
  return Number.isSafeInteger(value) && value > 0 ? value : 1;
}

export function selectCatalogPage(number: number, tab?: string) {
  const url = new URL(page.url.href);
  if (number > 1) url.searchParams.set("page", String(number));
  else url.searchParams.delete("page");
  if (tab !== undefined) url.searchParams.set("tab", tab);
  return goto(url, { reset: false });
}
