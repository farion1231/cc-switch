import React from "react";

export interface AuthCenterSectionEntry {
  id: string;
  Component: React.LazyExoticComponent<React.ComponentType>;
}

const sections: AuthCenterSectionEntry[] = [];

export function registerAuthCenterSection(entry: AuthCenterSectionEntry) {
  sections.push(entry);
}

export function getAuthCenterSections() {
  return sections;
}
