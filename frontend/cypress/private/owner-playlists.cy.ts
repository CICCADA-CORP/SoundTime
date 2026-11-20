export {};

type Account = { username: string; password: string };
type Playlist = { id: string; name: string; is_public: boolean };
type Fixture = {
  member: Account;
  other: Account;
  albumId: string;
  importPageThree: { id: string; title: string };
  tracks: { id: string; title: string }[];
  playlists: Playlist[];
};

function login(account: Account) {
  return cy.task<{ access_token: string; refresh_token: string }>('privateTokens', account.username, { log: false });
}

describe('Personal playlists on a private instance', () => {
  let created: string[] = [];
  let ownerHeaders: { Authorization: string };

  beforeEach(() => { created = []; });
  afterEach(() => {
    cy.wrap(created, { log: false }).each(id =>
      cy.request({ method: 'DELETE', url: '/api/playlists/' + id, headers: ownerHeaders, log: false })
        .its('status').should('eq', 204),
    );
  });

  it('opens the 101st personal import and keeps the page on reload', () => {
    cy.task<Fixture>('privateFixture', null, { log: false }).then(fixture => {
      login(fixture.other).then(tokens => {
        cy.visit('/library', { onBeforeLoad(win) {
          win.localStorage.setItem('soundtime_lang', 'en');
          win.localStorage.setItem('soundtime_access_token', tokens.access_token);
          win.localStorage.setItem('soundtime_refresh_token', tokens.refresh_token);
        } });
        cy.get('main button[aria-label=Next]').click();
        cy.location('search').should('include', 'page=2');
        cy.contains('main', '2 / 3').should('be.visible');
        cy.get('main button[aria-label=Next]').click();
        cy.location('search').should('include', 'page=3');
        cy.reload();
        cy.contains('main a', fixture.importPageThree.title).should('have.attr', 'href', '/tracks/' + fixture.importPageThree.id);
        cy.get('main button[aria-label=Next]').should('be.disabled');
        cy.go('back');
        cy.contains('main', '2 / 3').should('be.visible');
      });
    });
  });

  it('creates a private playlist, saves from page two, retries and enforces ownership', () => {
    cy.task<Fixture>('privateFixture', null, { log: false }).then(fixture => {
      const name = 'Private collection ' + crypto.randomUUID();
      let target: string;
      const track = fixture.tracks[0];

      cy.request({ url: '/api/me/playlists', failOnStatusCode: false }).its('status').should('eq', 401);
      login(fixture.member).then(tokens => {
        const headers = { Authorization: 'Bearer ' + tokens.access_token };
        ownerHeaders = headers;
        cy.visit('/library?tab=playlists', { onBeforeLoad(win) {
          win.localStorage.setItem('soundtime_lang', 'en');
          win.localStorage.setItem('soundtime_access_token', tokens.access_token);
          win.localStorage.setItem('soundtime_refresh_token', tokens.refresh_token);
        } });
        fixture.playlists.forEach(playlist => cy.contains('main a', playlist.name).should('exist'));
        cy.contains('main button', 'Create a playlist').click();
        cy.get('[role=dialog] input[name=name]').type(name);
        cy.get('[role=dialog] input[type=checkbox]').uncheck();
        cy.intercept('POST', '**/api/playlists').as('createCollection');
        cy.get('[role=dialog] button[type=submit]').click();
        cy.wait('@createCollection').then(({ response }) => {
          expect(response?.statusCode).to.equal(201);
          target = response!.body.id;
          created.push(target);
        });
        cy.contains('main h1', name).should('be.visible');

        // Put the new playlist exactly beyond the first page in updated order.
        cy.wrap(Array.from({ length: 20 }, (_, i) => i), { log: false }).each(i =>
          cy.request({
            method: 'POST', url: '/api/playlists', headers, log: false,
            body: { name: name + ' newer ' + i, is_public: false },
          }).then(response => { created.push(response.body.id); }),
        );

        cy.visit('/albums/' + fixture.albumId);
        cy.get('main [role=group]').filter((_, el) => el.getAttribute('aria-label') === track.title)
          .find('button[aria-haspopup=menu]').click();
        cy.contains('[role=menuitem]', 'Add to playlist').click();
        cy.get('[role=dialog] button[aria-label=Next]').click();
        cy.contains('[role=dialog] button', name).should('be.visible');

        // Lose the first response after the server has already saved the track.
        cy.intercept({ method: 'POST', url: '**/api/playlists/*/tracks', times: 1 }, req => {
          req.continue(res => res.send({ statusCode: 503, body: { error: 'Temporary response failure' } }));
        });
        cy.contains('[role=dialog] button', name).click();
        cy.get('[role=alert]').should('contain', 'Temporary response failure');
        cy.contains('[role=dialog] button', name).click();
        cy.get('[role=dialog]').should('not.exist');

        cy.visit('/library?tab=playlists&page=2');
        cy.reload();
        cy.contains('main a', name).click();
        cy.get('main [role=group]').filter((_, el) => el.getAttribute('aria-label') === track.title)
          .should('have.length', 1);

        cy.then(() => cy.request({ url: '/api/playlists/' + target, headers, log: false })
          .then(({ body }) => {
            expect(body.is_public).to.equal(false);
            expect(body.tracks.map((t: { id: string }) => t.id)).to.deep.equal([track.id]);
          }));

        login(fixture.other).then(other => {
          const headers = { Authorization: 'Bearer ' + other.access_token };
          cy.request({ url: '/api/me/playlists', headers, log: false })
            .then(({ body }) => expect(body.data.map((p: Playlist) => p.id)).not.to.include(target));
          cy.request({ url: '/api/playlists?per_page=100', headers, log: false })
            .then(({ body }) => {
              const ids = body.data.map((p: Playlist) => p.id);
              expect(ids).to.include(fixture.playlists.find(p => p.is_public)!.id);
              expect(ids).not.to.include(target);
            });
          cy.request({ url: '/api/playlists/' + target, headers, failOnStatusCode: false, log: false })
            .its('status').should('eq', 404);
          cy.request({ method: 'PUT', url: '/api/playlists/' + target, headers, body: { name: 'Denied' }, failOnStatusCode: false, log: false })
            .its('status').should('eq', 403);
          cy.request({ method: 'POST', url: '/api/playlists/' + target + '/tracks', headers, body: { track_id: track.id }, failOnStatusCode: false, log: false })
            .its('status').should('eq', 403);
        });
      });
    });
  });
});
