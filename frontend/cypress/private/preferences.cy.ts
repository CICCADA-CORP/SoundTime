export {};

type Account = { username: string; password: string };
type Fixture = { member: Account; admin: Account; albumId: string; tracks: { id: string }[] };

function visitAs(account: Account, path: string) {
  cy.task<{ access_token: string; refresh_token: string }>('privateTokens', account.username, { log: false }).then(tokens => {
    cy.visit(path, { onBeforeLoad(win) {
      win.localStorage.setItem('soundtime_lang', 'en');
      win.localStorage.setItem('soundtime_access_token', tokens.access_token);
      win.localStorage.setItem('soundtime_refresh_token', tokens.refresh_token);
    } });
  });
}

describe('Account and administration on a private instance', () => {
  it('saves listening preferences, returns focus on cancel, and signs out of media', () => {
    cy.task<Fixture>('privateFixture', null, { log: false }).then(fixture => {
      visitAs(fixture.member, '/albums/' + fixture.albumId);
      cy.contains('main button', 'Play All').click();
      cy.get('[data-testid=audio-player]').should('be.visible');
      const stream = '/api/tracks/' + fixture.tracks[0].id + '/stream';
      const range = { Range: 'bytes=0-12' };
      cy.request({ url: stream, headers: range, log: false }).its('status').should('eq', 206);
      cy.get('.workspace-header button[data-popover-trigger]').click();
      cy.get('[role=dialog] a[href="/settings"]').click();
      const audioSettings = () => cy.contains('main h2', /^Audio$/).closest('section');
      const volume = () => audioSettings().find('input[type=range][aria-label="Volume"]');
      const autoplay = () => audioSettings().find('label:contains("Autoplay") input[role=switch]');
      volume().invoke('val', 37).trigger('input');
      autoplay().should('have.length', 1).and('not.be.checked').click();
      cy.contains('main button', 'Delete my account').click();
      cy.contains('[role=dialog] button', 'Cancel').click();
      cy.focused().should('contain', 'Delete my account');
      cy.reload();
      volume().should('have.value', '37');
      autoplay().should('be.checked');
      cy.intercept('DELETE', '**/api/auth/media-session').as('clearGrant');
      cy.contains('main button', 'Logout').click();
      cy.wait('@clearGrant').its('response.statusCode').should('eq', 204);
      cy.location('pathname').should('eq', '/login');
      cy.request({ url: stream, headers: range, failOnStatusCode: false, log: false }).its('status').should('eq', 401);
      cy.get('[data-testid=audio-player]').should('not.exist');
    });
  });

  it('opens the storage settings as an administrator and guards a direct member visit', () => {
    cy.task<Fixture>('privateFixture', null, { log: false }).then(fixture => {
      visitAs(fixture.member, '/admin');
      cy.location('pathname').should('eq', '/');
      cy.viewport(390, 844);
      visitAs(fixture.admin, '/admin');
      cy.contains('main h1', 'Administration').should('be.visible');
      cy.get('main select[aria-label=Administration]').select('storage');
      cy.contains('main', 'Configuration').should('be.visible');
      cy.get('main select[aria-label=Administration] option[value=plugins]').should('exist');
      cy.get('main select[aria-label=Administration] option[value=themes]').should('exist');
    });
  });
});
