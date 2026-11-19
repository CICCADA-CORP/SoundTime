export {};

type CatalogFixture = {
  member: { username: string; password: string };
  artist: string;
  album: string;
  albumId: string;
  tracks: { id: string; title: string }[];
  playlists: { id: string; is_public: boolean }[];
  withdrawn: { id: string; title: string };
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
        cy.intercept('GET', '**/api/p2p/search?*').as('networkSearch');
        if (width === 390) {
          cy.contains('main [role=tab]', 'Genres').click();
          cy.location('search').should('eq', '?tab=genres');
          cy.reload();
          cy.contains('main [role=tab]', 'Genres').should('have.attr', 'aria-selected', 'true');
          cy.go('back');
          cy.location('pathname').should('eq', '/explore');
          cy.location('search').should('eq', '');
          cy.contains('main [role=tab]', 'Recently added').should('have.attr', 'aria-selected', 'true');
          cy.get('main form[role=search] input[name=q]').type(fixture.artist + '{enter}');
        } else {
          cy.get('body').trigger('keydown', { key: 'k', ctrlKey: true });
          cy.get('[role=dialog] input[type=search]').type(fixture.artist + '{enter}');
        }
        cy.location('pathname').should('eq', '/search');
        if (width === 390) {
          const network = 'main section[aria-label="Network Results"]';
          cy.contains(network + ' button', 'Search the wider network').should('be.visible');
          cy.get('@networkSearch.all').should('have.length', 0);
          cy.contains(network + ' button', 'Search the wider network').click();
          // P2P is disabled on this isolated instance. Local results must still work.
          cy.wait('@networkSearch').its('response.statusCode').should('eq', 503);
          cy.contains(network + ' button', 'Try Again').should('be.visible');
        }
        cy.get('main a[href^="/artists/"]').contains(fixture.artist).click();
        cy.contains('main h1', fixture.artist).should('be.visible');
        cy.get('main a[href="/albums/' + fixture.albumId + '"]').first().click();
        cy.contains('main h1', fixture.album).should('be.visible');
        cy.go('back');
        cy.contains('main h1', fixture.artist).should('be.visible');
        cy.go('back');
        cy.location('search').should(query => expect(new URLSearchParams(query).get('q')).to.eq(fixture.artist));
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

  it('keeps a withdrawn playlist entry visible and skips its stream on mobile', () => {
    cy.viewport(390, 844);
    cy.task<CatalogFixture>('privateFixture', null, { log: false }).then(fixture => {
      const playlist = fixture.playlists.find(entry => !entry.is_public)!;
      const [first, next] = fixture.tracks;
      cy.task<{ access_token: string; refresh_token: string }>('privateTokens', fixture.member.username, { log: false }).then(tokens => {
        cy.visit('/playlists/' + playlist.id, { onBeforeLoad(win) {
          win.localStorage.setItem('soundtime_lang', 'en');
          win.localStorage.setItem('soundtime_access_token', tokens.access_token);
          win.localStorage.setItem('soundtime_refresh_token', tokens.refresh_token);
        } });
      });
      const rows = 'main [role=group][data-availability]';
      cy.get(rows).should('have.length', 3).eq(1).should('have.attr', 'aria-label', fixture.withdrawn.title);
      cy.get(rows).eq(1).should('have.attr', 'data-availability', 'withdrawn');
      cy.get(rows).eq(1).find('button.play-cell').should('be.disabled');
      cy.get(rows).eq(1).should('contain', 'Removed by the publisher');
      cy.intercept('GET', '**/api/tracks/' + fixture.withdrawn.id + '/stream*').as('withdrawnStream');
      cy.intercept('GET', '**/api/tracks/' + first.id + '/stream*').as('firstStream');
      cy.intercept('GET', '**/api/tracks/' + next.id + '/stream*').as('nextStream');
      cy.contains('main button', 'Play All').click();
      cy.wait('@firstStream').its('response.statusCode').should('be.oneOf', [200, 206]);
      cy.get('[data-testid=audio-player] button.open-player').click();
      cy.get('[role=dialog] button[aria-label="Next"]:visible').first().click();
      cy.wait('@nextStream').its('response.statusCode').should('be.oneOf', [200, 206]);
      cy.contains('[role=dialog] h2', next.title).should('be.visible');
      cy.get('@withdrawnStream.all').should('have.length', 0);
    });
  });
});
