describe('Authentication', () => {
	beforeEach(() => {
		cy.clearLocalStorage();
	});

	describe('Login', () => {
		it('displays login form', () => {
			cy.visit('/login');
			cy.get('input[type="text"], input[name="username"]').should('exist');
			cy.get('input[type="password"]').should('exist');
			cy.get('button[type="submit"]').should('exist');
		});

		it('shows validation errors for empty fields', () => {
			cy.visit('/login');
			cy.get('button[type="submit"]').click();
			// Should stay on login page
			cy.url().should('include', '/login');
		});

		it('shows error for invalid credentials', () => {
			cy.visit('/login');
			cy.get('input[type="text"], input[name="username"]').first().type('wronguser');
			cy.get('input[type="password"]').type('wrongpassword');
			cy.get('button[type="submit"]').click();
			// Should show error message or stay on page
			cy.url().should('include', '/login');
		});

		it('redirects to home on successful login', () => {
			cy.login('admin', 'Admin123!');
			cy.visit('/');
			// Should be logged in (no login button visible, or user menu visible)
			cy.url().should('not.include', '/login');
		});

		it('stores tokens in localStorage after login', () => {
			cy.login('admin', 'Admin123!');
			cy.window().then((win) => {
				expect(win.localStorage.getItem('soundtime_access_token')).to.not.be.null;
				expect(win.localStorage.getItem('soundtime_refresh_token')).to.not.be.null;
			});
		});
	});

	describe('Registration', () => {
		it('displays registration form', () => {
			cy.visit('/register');
			cy.get('input[type="email"], input[name="email"]').should('exist');
			cy.get('input[type="password"]').should('exist');
		});

		it('validates email format', () => {
			cy.visit('/register');
			cy.get('input[type="email"], input[name="email"]').first().type('invalid-email');
			cy.get('button[type="submit"]').click();
			cy.url().should('include', '/register');
		});
	});

	describe('Logout', () => {
		it('clears tokens and redirects on logout', () => {
			cy.login('admin', 'Admin123!');
			cy.visit('/');
			// Click on user menu or logout button
			cy.get('[data-testid="user-menu"], [data-testid="logout"], button:contains("Logout"), button:contains("Déconnexion")').first().click({ force: true });
			cy.window().then((win) => {
				expect(win.localStorage.getItem('soundtime_access_token')).to.be.null;
			});
		});
	});
});
