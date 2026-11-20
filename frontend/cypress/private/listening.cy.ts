type Fixture = {
	member: { username: string; password: string };
	tracks: { id: string; title: string }[];
	album: string;
	albumId: string;
};

describe('Private listening through the production proxy', () => {
	let fixture: Fixture;
	let audio: HTMLMediaElement;
	const player = '[data-testid="audio-player"]';

	beforeEach(() => {
		cy.task<Fixture>('privateFixture', null, { log: false }).then(data => { fixture = data; });
		cy.on('window:before:load', win => {
			win.localStorage.setItem('soundtime_lang', 'en');
			// Capture the real detached Audio element without changing its playback.
			const prototype = win.HTMLMediaElement.prototype;
			const play = prototype.play;
			prototype.play = function () { audio = this; return play.call(this); };
			const match = win.matchMedia.bind(win);
			win.matchMedia = query => {
				const media = match(query);
				if (query === '(prefers-reduced-motion: reduce)') Object.defineProperty(media, 'matches', { value: true });
				return media;
			};
		});
	});

	for (const width of [1440, 390]) {
		it('plays, seeks and keeps the same recording across navigation at ' + width + 'px', () => {
			cy.viewport(width, width === 390 ? 844 : 900);
			cy.request({ url: '/api/tracks/' + fixture.tracks[0].id + '/stream', failOnStatusCode: false, log: false })
				.its('status').should('eq', 401);
			cy.visit('/albums/' + fixture.albumId);
			cy.location('pathname').should('eq', '/login');
			cy.get('input[name=username]').should('be.visible');
			cy.screenshot('private-' + width + '-login', { capture: 'viewport' });
			cy.get('input[name=username]').type(fixture.member.username);
			cy.get('input[name=password]').type(fixture.member.password, { log: false });
			cy.get('form button[type=submit]').click();
			cy.location('pathname').should('eq', '/albums/' + fixture.albumId);
			cy.contains('main h1', fixture.album).should('be.visible');
			cy.intercept('GET', '**/api/tracks/*/stream*').as('stream');
			cy.contains('main button', 'Play All').click();
			cy.wait('@stream').then(({ request, response }) => {
				expect(new URL(request.url).search).to.eq('');
				expect(response?.statusCode).to.be.oneOf([200, 206]);
			});
			cy.wrap(null).should(() => { expect(Boolean(audio), 'native audio captured').to.eq(true); expect(audio.paused).to.eq(false); expect(audio.currentTime).to.be.greaterThan(0.2); });
			let original: HTMLMediaElement;
			let trackUrl: string;
			cy.then(() => { original = audio; trackUrl = audio.currentSrc; expect(trackUrl).to.include('/stream'); });
			cy.get(player + ' button.open-player').click();
			cy.get('[role=dialog] button[aria-label="Pause"]:visible').first().click();
			cy.wrap(null).should(() => expect(audio.paused).to.eq(true));
			cy.get('[role=dialog] input[type=range][aria-label="Playback position"]:visible')
				.invoke('val', 7).trigger('input');
			cy.wrap(null).should(() => expect(audio.currentTime).to.be.closeTo(7, 0.5));
			cy.get('body').type('{esc}');
			cy.get('[role=dialog]').should('not.exist');
			cy.then(() => cy.request({ url: trackUrl, headers: { Range: 'bytes=64-191' }, encoding: 'binary', log: false })
				.then(({ status, headers, body }) => {
					expect(status).to.eq(206); expect(headers['content-range']).to.match(/^bytes 64-191\//);
					expect(body.length).to.eq(128);
				}));
			cy.get('a[data-workspace-home]:visible').click();
			cy.location('pathname').should('eq', '/');
			cy.get('main section h2').first().should('be.visible');
			cy.wrap(null).should(() => {
				expect(audio).to.eq(original); expect(audio.currentSrc).to.eq(trackUrl); expect(audio.currentTime).to.be.closeTo(7, 0.5);
			});
			cy.get(player).should('be.visible');
			cy.screenshot('private-' + width + '-home-player', { capture: 'viewport' });
			cy.get(player + ' button.open-player').click();
			cy.get('[role=dialog]').should('be.visible');
			cy.screenshot('private-' + width + '-expanded', { capture: 'viewport' });
			cy.get('[role=dialog] button[aria-label="Play"]:visible').first().click();
			cy.wrap(null).should(() => expect(audio.currentTime).to.be.greaterThan(7.2));
			// Expire the browser grant while the saved queue still contains another recording.
			cy.request({ method: 'DELETE', url: '/api/auth/media-session', log: false });
			cy.get('[role=dialog] button[aria-label="Next"]:visible').first().click();
			cy.get('[role=dialog] button[aria-label="Try Again"]:visible').click();
			cy.wrap(null).should(() => {
				expect(audio).to.eq(original);
				expect(audio.currentSrc).not.to.eq(trackUrl);
				expect(audio.paused).to.eq(false);
				expect(audio.currentTime).to.be.greaterThan(0.2);
			});

		});
	}
});
