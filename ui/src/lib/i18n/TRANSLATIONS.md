Only English, French, and German are active right now because they are the langauges i can confrim for the others a native speaker is needed
 (Spanish, Portuguese, Japanese, Chinese, Arabic(will maybe be removed unless i decide there is a use to it)) are kept in the repository as dormant code until a native speaker can review and confirm them. They won't appear in the UI language selector until then.

How to Contribute or Activate a Language
If you or acontributor want to verify and enable a language:

Verify: Have a native speaker double-check all translations for accuracy.

Define Locale: Add the language code and its display label to LOCALES and LOCALE_LABELS in locales.ts.

Register Catalog: Import the translation catalog and add it to the CATALOGS record in index.ts.

Layout (RTL): If the language is right-to-left, add its code to RTL_LOCALES in locales.ts.