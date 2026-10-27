// Tipi della libreria degli add-on di Postamon (API 1), per l'editor:
// copia questo file accanto ai tuoi script, oppure aggiungi in cima
// `/// <reference path="../../sdk/postamon-addon.d.ts" />`.

declare namespace Postamon {
  type Hook = "message-button" | "sidebar-view";

  interface Context {
    hook: Hook;
    addon: { id: string; version: string };
    api: number;
    /** C'è un messaggio aperto da leggere o a cui rispondere. */
    message: boolean;
  }

  interface Person {
    name: string;
    address: string;
  }

  interface Attachment {
    index: number;
    name: string;
    mime: string;
    size: number;
  }

  interface Message {
    subject: string;
    from: Person;
    to: Person[];
    cc: Person[];
    /** Secondi dal 1970, se noti. */
    date: number | null;
    /** La data scritta in italiano ("26 ottobre 2026, 10:30"). */
    dateText: string;
    /** Il testo del messaggio, senza HTML. */
    text: string;
    attachments: Attachment[];
  }

  interface Draft {
    to?: string;
    cc?: string;
    subject?: string;
    body?: string;
  }

  interface ReplyDraft {
    /** Testo da mettere sopra la citazione. */
    body?: string;
    /** Rispondi a tutti. */
    all?: boolean;
  }

  type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

  interface Api {
    readonly api: 1;
    context(): Promise<Context>;
    readonly message: { read(): Promise<Message> };
    readonly compose: {
      open(draft?: Draft): Promise<void>;
      reply(draft?: ReplyDraft): Promise<void>;
    };
    readonly storage: {
      get<T extends Json = Json>(key: string): Promise<T | null>;
      set(key: string, value: Json): Promise<void>;
      remove(key: string): Promise<void>;
      keys(): Promise<string[]>;
    };
    readonly ui: {
      toast(text: string, kind?: "info" | "warn" | "error"): Promise<void>;
      close(): Promise<void>;
    };
  }
}

declare const postamon: Postamon.Api;
