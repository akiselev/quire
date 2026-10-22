import fs from 'node:fs';

const MAX_SIZE = 10;

export class Store extends Map {
  get size() { return super.size; }
}

function greet(name, { loud = false } = {}) {
  const msg = `Hello, ${name}!`;
  console.log(loud ? msg.toUpperCase() : msg);
  return typeof name === 'string' && name.length < MAX_SIZE;
}

const styles = css`
  .btn { color: blue; }
`;

const el = <div className="x">{greet('a')}</div>;
