describe("Settings", () => {
  beforeEach(() => {
    cy.loginAsAdmin();
    cy.visit("/settings", {
      onBeforeLoad(win) { win.localStorage.setItem("soundtime_lang", "en"); },
    });
  });

  it("persists the volume, autoplay and language changed from the preferences", () => {
    cy.get('input[type="range"][aria-label="Volume"]').as("volume");
    cy.get("@volume").invoke("val", "32").trigger("input");
    cy.contains("label", "Autoplay").find('[role="switch"]').check();
    cy.reload();
    cy.get('input[type="range"][aria-label="Volume"]').should("have.value", "32");
    cy.contains("label", "Autoplay").find('[role="switch"]').should("be.checked");
    cy.contains("label", "Display Language").find("select").select("fr");
    cy.reload();
    cy.get("main select").should("have.value", "fr");
  });

  it("cancels the account confirmation and returns focus to its trigger", () => {
    cy.contains("button", "Delete my account").as("trigger").click();
    cy.get('[role="dialog"]').should("be.visible").contains("button", "Cancel").click();
    cy.get('[role="dialog"]').should("not.exist");
    cy.get("@trigger").should("be.focused");
  });
});
