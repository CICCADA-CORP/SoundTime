describe('Playlists', () => {
	beforeEach(() => {
		cy.loginAsAdmin();
	});

	it('shows playlists page', () => {
		cy.visit('/playlists');
		cy.url().should('include', '/playlists');
	});

	it('can create a new playlist via API', () => {
		cy.window().then((win) => {
			const token = win.localStorage.getItem('soundtime_access_token');
			cy.request({
				method: 'POST',
				url: '/api/playlists',
				headers: {
					Authorization: `Bearer ${token}`,
					'Content-Type': 'application/json'
				},
				body: {
					name: `Test Playlist ${Date.now()}`,
					description: 'Created by E2E test',
					is_public: true
				},
				failOnStatusCode: false
			}).then((res) => {
				expect(res.status).to.be.oneOf([200, 201]);
				if (res.body.id) {
					// Cleanup: delete the playlist
					cy.request({
						method: 'DELETE',
						url: `/api/playlists/${res.body.id}`,
						headers: { Authorization: `Bearer ${token}` },
						failOnStatusCode: false
					});
				}
			});
		});
	});

	it('playlist detail page loads', () => {
		cy.window().then((win) => {
			const token = win.localStorage.getItem('soundtime_access_token');
			cy.request({
				url: '/api/playlists?per_page=1',
				headers: { Authorization: `Bearer ${token}` },
				failOnStatusCode: false
			}).then((res) => {
				if (res.status === 200 && res.body.data?.length > 0) {
					const playlistId = res.body.data[0].id;
					cy.visit(`/playlists/${playlistId}`);
					cy.url().should('include', `/playlists/${playlistId}`);
				}
			});
		});
	});

	it('public playlists endpoint works', () => {
		cy.request({
			url: '/api/playlists',
			failOnStatusCode: false
		}).then((res) => {
			expect(res.status).to.be.oneOf([200, 401]);
		});
	});
});
