const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const read = (f) => fs.readFileSync(path.join(__dirname, '../ui', f), 'utf8');

// i18n.js in one language, its text read from the real language file.
async function inLang(code) {
  const window = {};
  const text = JSON.parse(read(`i18n/${code}.json`));
  const fetch = async () => ({ ok: true, json: async () => text });
  const document = { documentElement: {} };
  vm.runInNewContext(read('i18n.js'), { window, navigator: { languages: [code] }, document, fetch });
  await window.loadLang(code);
  return window.tf;
}

const PEOPLE = '{n} people saved it or are waiting for it';

test('Russian picks the form for 1, 21, 3 and 5 people', async () => {
  const tf = await inLang('ru');
  assert.equal(tf(PEOPLE, { n: 1 }), '1 человек сохранил её или ждёт');
  assert.equal(tf(PEOPLE, { n: 21 }), '21 человек сохранил её или ждёт');
  assert.equal(tf(PEOPLE, { n: 3 }), '3 человека сохранили её или ждут');
  assert.equal(tf(PEOPLE, { n: 5 }), '5 человек сохранили её или ждут');
});

test('Polish takes the one form for 1 only, and the few form for 22', async () => {
  const tf = await inLang('pl');
  assert.equal(tf(PEOPLE, { n: 1 }), '1 osoba zapisała go lub na niego czeka');
  assert.equal(tf(PEOPLE, { n: 21 }), '21 osób zapisało go lub na niego czeka');
  assert.equal(tf(PEOPLE, { n: 22 }), '22 osoby zapisały go lub na niego czekają');
});

test('Ukrainian takes the one form for 21 games in the saves note', async () => {
  const tf = await inLang('uk');
  const many = '{n} more games have no saves yet. Play them once and their saves show up here.';
  assert.match(tf(many, { n: 21 }), /^Ще в 21 гри /);
  assert.match(tf(many, { n: 2 }), /^Ще в 2 ігор /);
});

test('a single form is kept for every count', async () => {
  const tf = await inLang('ru');
  assert.equal(tf('Disc {n}', { n: 2 }), 'Диск 2');
  assert.equal(tf('Disc {n}', { n: 5 }), 'Диск 5');
});
