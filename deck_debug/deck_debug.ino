// Pin finder: prints which Nano pin goes LOW when you press a button.
// Serial Monitor at 115200. Every input pin uses its internal pull-up, so a
// button wired between the pin and GND shows up as "D5 PRESSED" etc.
// Encoder turns show as its two pins toggling; encoder push shows as a press.
// D0/D1 are skipped (USB serial). D13 has the on-board LED and may read LOW
// constantly on some clones - ignore it if so. A6/A7 are analog-only on the
// Nano (no pull-up), so they are not scanned.
// Send '?' and it answers "PONG consoledeck" (used by deck.py doctor).

const uint8_t pins[] = {2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13,
                        A0, A1, A2, A3, A4, A5};
const uint8_t N = sizeof(pins);
const unsigned long DEBOUNCE_MS = 20;

uint8_t state[N];
unsigned long lastChange[N];

void printName(uint8_t pin) {
  if (pin >= A0) { Serial.print('A'); Serial.print(pin - A0); }
  else { Serial.print('D'); Serial.print(pin); }
}

void setup() {
  Serial.begin(115200);
  for (uint8_t i = 0; i < N; i++) pinMode(pins[i], INPUT_PULLUP);
  delay(50);
  Serial.println(F("\n=== Pin finder: press a button ==="));
  for (uint8_t i = 0; i < N; i++) {
    state[i] = digitalRead(pins[i]);
    if (state[i] == LOW) {
      printName(pins[i]);
      Serial.println(F(" is LOW at boot (held, shorted to GND, or encoder resting)"));
    }
  }
}

void loop() {
  unsigned long now = millis();
  for (uint8_t i = 0; i < N; i++) {
    uint8_t v = digitalRead(pins[i]);
    if (v != state[i] && now - lastChange[i] > DEBOUNCE_MS) {
      state[i] = v;
      lastChange[i] = now;
      printName(pins[i]);
      Serial.println(v == LOW ? F(" PRESSED") : F(" released"));
    }
  }
  if (Serial.available() && Serial.read() == '?') Serial.println(F("PONG consoledeck"));  // health ping for deck.py doctor
}
