export {};

type CatalogFixture = {
  member: { username: string; password: string };
  artist: string;
  album: string;
  albumId: string;
  tracks: { id: string; title: string }[];
};

describe('Catalog navigation and favorites', () => {
  for (const width of [390, 1440]) {
    it('finds an artist, plays an album track and retrieves the favorite at ' + width + 'px', () => {
      cy.viewport(width, width === 390 ? 844 : 900);
      cy.task<CatalogFixture>('privateFixture', null, { log: false }).then(fixture => {
        const track = fixture.tracks[0];
        cy.task<{ access_token: string; refresh_token: string }>('privateTokens', fixture.member.username, { log: false }).then(tokens => {
          const headers = { Authorization: 'Bearer ' + tokens.access_token };
          cy.request({ method: 'DELETE', url: '/api/favorites/' + track.id, headers, log: false });
          cy.visit('/explore', { onBeforeLoad(win) {
            win.localStorage.setItem('soundtime_lang', 'en');
            win.localStorage.setItem('soundtime_access_token', tokens.access_token);
            win.localStorage.setItem('soundtime_refresh_token', tokens.refresh_token);
          } });
        });
        cy.get('main h1').should('contain.text', 'Explore');
        if (width === 390) cy.get('header button[aria-label=Search]').click();
        else cy.get('body').trigger('keydown', { key: 'k', ctrlKey: true });
        cy.get('[role=dialog] input[type=search]').type(fixture.artist + '{enter}');
        cy.location('pathname').should('eq', '/search');
        cy.get('main a[href^="/artists/"]').contains(fixture.artist).click();
        cy.contains('main h1', fixture.artist).should('be.visible');
        cy.get('main a[href="/albums/' + fixture.albumId + '"]').first().click();
        cy.contains('main h1', fixture.album).should('be.visible');
        cy.go('back');
        cy.contains('main h1', fixture.artist).should('be.visible');
        cy.go('back');
        cy.location('search').should('include', encodeURIComponent(fixture.artist));
        cy.get('main a[href^="/artists/"]').contains(fixture.artist).click();
        cy.get('main a[href="/albums/' + fixture.albumId + '"]').first().click();
        cy.get('main [role=group]').filter((_, el) => el.getAttribute('aria-label') === track.title)
          .find('button.play-cell').click();
        cy.contains('[data-testid=audio-player]', track.title).should('be.visible');
        cy.get('main [role=group]').filter((_, el) => el.getAttribute('aria-label') === track.title)
          .find('button[aria-pressed=false]').click();
        cy.get('main [role=group]').filter((_, el) => el.getAttribute('aria-label') === track.title)
          .find('button[aria-pressed=true]').should('be.visible');
        if (width === 390) cy.get('nav.mobile-tabs a[href="/library"]').click();
        cy.get('a[href="/favorites"]:visible').first().click();
        cy.contains('main a', track.title).should('have.attr', 'href', '/tracks/' + track.id);
        cy.get('main [role=group]').filter((_, el) => el.getAttribute('aria-label') === track.title)
          .find('button[aria-pressed=true]').click();
        cy.contains('main a', track.title).should('not.exist');
        cy.contains('[data-testid=audio-player]', track.title).should('be.visible');
      });
    });
  }
});
