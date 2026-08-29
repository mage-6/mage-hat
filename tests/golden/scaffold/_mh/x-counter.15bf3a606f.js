customElements.define('x-counter', class extends HTMLElement {
    connectedCallback() {
      const out = this.querySelector('span');
      this.querySelector('button').addEventListener('click', () => {
        out.textContent = Number(out.textContent) + 1;
      });
    }
  });
